//! 模拟 BMC：按厂商画像模拟资源布局与差异（`VirtualMedia` 在 System/Manager 下、需先弹出、202 + Task、
//! 401、`ExtendedInfo` 错误、`Inserted` 延迟、拒绝 `BootSourceOverrideMode`、Legacy 模式、已关机）。

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::server::{Handler, Incoming, Outgoing};

/// 会话令牌。
const TOKEN: &str = "tok-1";
/// `admin:secret` 的 Basic 认证头。
const BASIC: &str = "Basic YWRtaW46c2VjcmV0";
/// 任务监视器路径。
const MONITOR: &str = "/redfish/v1/TaskService/TaskMonitors/7";

/// 行为差异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Quirk {
    /// `VirtualMedia` 挂在 `Managers/1` 下。
    ManagerVmedia,
    /// 没有 `SessionService`（只能 Basic）。
    NoSessions,
    /// 初始已插入其它介质，且已插入时拒绝 `InsertMedia`（必须先弹出）。
    MustEject,
    /// `InsertMedia` 返回 202 + `TaskMonitor`。
    Task202,
    /// 插入后前 N 次读取仍报告 `Inserted=false`。
    InsertDelay(u32),
    /// `PATCH Boot` 带 `BootSourceOverrideMode` 时返回 400。
    RejectBootMode,
    /// 当前启动模式为 Legacy。
    Legacy,
    /// `InsertMedia` 返回带 `ExtendedInfo` 的 400。
    InsertError,
    /// 已关机。
    PoweredOff,
}

/// 模拟状态。
#[derive(Debug)]
pub(crate) struct State {
    /// `ServiceRoot.Vendor`。
    vendor: String,
    /// 行为差异。
    quirks: Vec<Quirk>,
    /// 是否已插入。
    inserted: bool,
    /// 插入后读取次数。
    reads: u32,
    /// 任务轮询次数。
    pub(crate) task_polls: u32,
    /// 请求日志。
    pub(crate) log: Vec<Incoming>,
}

impl State {
    /// 是否具有某差异。
    fn has(&self, quirk: Quirk) -> bool {
        self.quirks.contains(&quirk)
    }

    /// 插入延迟。
    fn delay(&self) -> u32 {
        self.quirks
            .iter()
            .find_map(|quirk| match quirk {
                Quirk::InsertDelay(count) => Some(*count),
                Quirk::ManagerVmedia
                | Quirk::NoSessions
                | Quirk::MustEject
                | Quirk::Task202
                | Quirk::RejectBootMode
                | Quirk::Legacy
                | Quirk::InsertError
                | Quirk::PoweredOff => None,
            })
            .unwrap_or(0)
    }

    /// `VirtualMedia` 集合路径。
    fn vm_base(&self) -> &'static str {
        if self.has(Quirk::ManagerVmedia) {
            "/redfish/v1/Managers/1/VirtualMedia"
        } else {
            "/redfish/v1/Systems/1/VirtualMedia"
        }
    }
}

/// 共享状态。
pub(crate) type Shared = Arc<Mutex<State>>;

/// 创建模拟 BMC，返回状态与处理函数。
pub(crate) fn mock(vendor: &str, quirks: &[Quirk]) -> (Shared, Handler) {
    let state = Arc::new(Mutex::new(State {
        vendor: vendor.to_owned(),
        quirks: quirks.to_vec(),
        inserted: quirks.contains(&Quirk::MustEject),
        reads: 0,
        task_polls: 0,
        log: Vec::new(),
    }));
    let shared = Arc::clone(&state);
    let handler: Handler = Arc::new(move |request: &Incoming| {
        shared.lock().map_or_else(
            |_poisoned| (500, Vec::new(), Value::Null),
            |mut guard| handle(&mut guard, request),
        )
    });
    (state, handler)
}

/// 简单响应。
fn reply(status: u16, body: Value) -> Outgoing {
    (status, Vec::new(), body)
}

/// 带 `ExtendedInfo` 的错误响应。
fn error(status: u16, id: &str, message: &str) -> Outgoing {
    reply(
        status,
        json!({"error": {"code": "Base.1.0.GeneralError", "message": "A general error has occurred.",
            "@Message.ExtendedInfo": [{"MessageId": id, "Message": message}]}}),
    )
}

/// 请求是否已认证。
fn authorized(request: &Incoming) -> bool {
    request.authorization.as_deref() == Some(BASIC) || request.token.as_deref() == Some(TOKEN)
}

/// 路由。
fn handle(state: &mut State, request: &Incoming) -> Outgoing {
    state.log.push(request.clone());
    let route = (request.method.as_str(), request.path.as_str());
    match route {
        ("GET", "/redfish/v1") => reply(200, root(state)),
        ("POST", "/redfish/v1/SessionService/Sessions") => login(state, request),
        _other if !authorized(request) => error(401, "Base.1.0.NoValidSession", "No valid session"),
        _other => resources(state, request),
    }
}

/// `ServiceRoot`。
fn root(state: &State) -> Value {
    let mut root = json!({"@odata.id": "/redfish/v1", "RedfishVersion": "1.15.0", "Vendor": state.vendor,
        "Systems": {"@odata.id": "/redfish/v1/Systems"}, "Managers": {"@odata.id": "/redfish/v1/Managers"}});
    if !state.has(Quirk::NoSessions)
        && let Some(object) = root.as_object_mut()
    {
        let _service = object.insert(
            "SessionService".to_owned(),
            json!({"@odata.id": "/redfish/v1/SessionService"}),
        );
        let _links = object.insert(
            "Links".to_owned(),
            json!({"Sessions": {"@odata.id": "/redfish/v1/SessionService/Sessions"}}),
        );
    }
    root
}

/// 创建会话。
fn login(state: &State, request: &Incoming) -> Outgoing {
    if state.has(Quirk::NoSessions) {
        return reply(405, Value::Null);
    }
    let user = request.body.get("UserName").and_then(Value::as_str);
    let pass = request.body.get("Password").and_then(Value::as_str);
    if user != Some("admin") || pass != Some("secret") {
        return error(
            401,
            "Base.1.0.InvalidCredentials",
            "Invalid username or password",
        );
    }
    (
        201,
        vec![
            ("X-Auth-Token".to_owned(), TOKEN.to_owned()),
            (
                "Location".to_owned(),
                "/redfish/v1/SessionService/Sessions/1".to_owned(),
            ),
        ],
        json!({"Id": "1"}),
    )
}

/// 已认证资源。
fn resources(state: &mut State, request: &Incoming) -> Outgoing {
    let base = state.vm_base();
    let path = request.path.as_str();
    match request.method.as_str() {
        "GET" if path.starts_with(base) => {
            media(state, path.strip_prefix(base).unwrap_or_default())
        }
        "GET" => get(state, path),
        "POST" if path == format!("{base}/Cd/Actions/VirtualMedia.InsertMedia") => insert(state),
        "POST" if path == format!("{base}/Cd/Actions/VirtualMedia.EjectMedia") => {
            state.inserted = false;
            reply(204, Value::Null)
        }
        "POST" if path == "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset" => {
            reply(204, Value::Null)
        }
        "PATCH" if path == "/redfish/v1/Systems/1" => patch(state, request),
        "DELETE" if path == "/redfish/v1/SessionService/Sessions/1" => reply(204, Value::Null),
        _other => reply(404, Value::Null),
    }
}

/// 非介质 GET。
fn get(state: &mut State, path: &str) -> Outgoing {
    match path {
        "/redfish/v1/Systems" => reply(
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Systems/1"}]}),
        ),
        "/redfish/v1/Systems/1" => reply(200, system(state)),
        "/redfish/v1/Managers" => reply(
            200,
            json!({"Members": [{"@odata.id": "/redfish/v1/Managers/1"}]}),
        ),
        "/redfish/v1/Managers/1" if state.has(Quirk::ManagerVmedia) => reply(
            200,
            json!({"Id": "1", "VirtualMedia": {"@odata.id": "/redfish/v1/Managers/1/VirtualMedia"}}),
        ),
        "/redfish/v1/Managers/1" => reply(200, json!({"Id": "1"})),
        MONITOR => {
            state.task_polls = state.task_polls.saturating_add(1);
            if state.task_polls < 2 {
                reply(202, json!({"TaskState": "Running"}))
            } else {
                reply(200, json!({"TaskState": "Completed"}))
            }
        }
        _other => reply(404, Value::Null),
    }
}

/// `ComputerSystem`。
fn system(state: &State) -> Value {
    let mode = if state.has(Quirk::Legacy) {
        "Legacy"
    } else {
        "UEFI"
    };
    let power = if state.has(Quirk::PoweredOff) {
        "Off"
    } else {
        "On"
    };
    let mut body = json!({"Id": "1", "PowerState": power,
        "Boot": {"BootSourceOverrideTarget": "None", "BootSourceOverrideMode": mode,
            "BootSourceOverrideTarget@Redfish.AllowableValues": ["None", "Pxe", "Cd", "Hdd"]}});
    if !state.has(Quirk::ManagerVmedia)
        && let Some(object) = body.as_object_mut()
    {
        let _link = object.insert(
            "VirtualMedia".to_owned(),
            json!({"@odata.id": "/redfish/v1/Systems/1/VirtualMedia"}),
        );
    }
    body
}

/// 介质资源（`rest` 为集合路径之后的部分）。
fn media(state: &mut State, rest: &str) -> Outgoing {
    let base = state.vm_base();
    match rest {
        "" => reply(
            200,
            json!({"Members": [{"@odata.id": format!("{base}/Floppy1")}, {"@odata.id": format!("{base}/Cd")}]}),
        ),
        "/Floppy1" => reply(
            200,
            json!({"Id": "Floppy1", "MediaTypes": ["Floppy", "USBStick"]}),
        ),
        "/Cd" => {
            if state.inserted {
                state.reads = state.reads.saturating_add(1);
            }
            let visible = state.inserted && state.reads > state.delay();
            reply(
                200,
                json!({"Id": "Cd", "MediaTypes": ["CD", "DVD"], "Inserted": visible}),
            )
        }
        _other => reply(404, Value::Null),
    }
}

/// `InsertMedia`。
fn insert(state: &mut State) -> Outgoing {
    if state.has(Quirk::InsertError) {
        return error(
            400,
            "Base.1.8.ActionParameterValueFormatError",
            "Image URL is not reachable.",
        );
    }
    if state.has(Quirk::MustEject) && state.inserted {
        return error(400, "Base.1.8.ResourceInUse", "Media already inserted.");
    }
    state.inserted = true;
    state.reads = 0;
    if state.has(Quirk::Task202) {
        return (
            202,
            vec![("Location".to_owned(), MONITOR.to_owned())],
            Value::Null,
        );
    }
    reply(204, Value::Null)
}

/// `PATCH Boot`。
fn patch(state: &State, request: &Incoming) -> Outgoing {
    let has_mode = request
        .body
        .pointer("/Boot/BootSourceOverrideMode")
        .is_some();
    if has_mode && state.has(Quirk::RejectBootMode) {
        return error(
            400,
            "Base.1.8.PropertyUnknown",
            "BootSourceOverrideMode is not supported.",
        );
    }
    reply(204, Value::Null)
}

/// 日志中匹配方法与路径后缀的请求。
pub(crate) fn find(state: &Shared, method: &str, suffix: &str) -> Vec<Incoming> {
    state.lock().map_or_else(
        |_poisoned| Vec::new(),
        |guard| {
            guard
                .log
                .iter()
                .filter(|item| item.method == method && item.path.ends_with(suffix))
                .cloned()
                .collect()
        },
    )
}
