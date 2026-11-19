// 设置（step-12 §3.10）：我的账户（改密、会话、API Token）、用户管理（admin）、装机超时、启动配置（只读）、外观。
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { CopyButton } from "../../../components/ui/code";
import { Checkbox, NumberField } from "../../../components/ui/controls";
import { DataTable } from "../../../components/ui/data-table";
import { AlertDialog, Dialog } from "../../../components/ui/dialog";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Field, Input } from "../../../components/ui/field";
import { Card, CardLabel, Section } from "../../../components/ui/layout";
import { Select } from "../../../components/ui/select";
import { api } from "../../../lib/api";
import { fmtTime } from "../../../lib/format";
import type { Me, SessionInfo, SystemInfo, TokenInfo, UserSummary } from "../../../lib/types";
import { useTheme } from "../../../theme/ThemeProvider";
import { AsyncView } from "../../AsyncView";
import { PasswordForm } from "../../PasswordForm";
import { useAction, useMe, useSessions, useSystemInfo, useTokens, useUsers } from "../../data";
import { SingletonSave, useSingleton } from "../configs/singleton";

const ROLES = [
  { value: "admin", label: "管理员" },
  { value: "operator", label: "操作员" },
  { value: "viewer", label: "只读" },
];
const SCOPES = ["read", "operate", "admin"] as const;

function Sessions() {
  const query = useSessions();
  const { perform } = useAction();
  return (
    <AsyncView query={query} isEmpty={(rows) => rows.length === 0} empty={{ title: "没有活动会话", description: "" }}>
      {(rows) => (
        <DataTable<SessionInfo>
          id="sessions"
          label="活动会话"
          rows={rows}
          rowKey={(row) => row.key}
          columns={[
            { id: "ip", header: "IP", cell: (row) => row.ip, data: true, hideable: false },
            { id: "ua", header: "客户端", cell: (row) => <span className="truncate-cell">{row.user_agent}</span> },
            { id: "seen", header: "最近活动", cell: (row) => fmtTime(row.last_seen_at), sortValue: (row) => row.last_seen_at, data: true },
            { id: "current", header: "", cell: (row) => (row.current ? <Badge tone="success">当前</Badge> : null) },
          ]}
          rowMenu={(row) => [{ kind: "item", value: "revoke", label: "注销", danger: true, disabled: row.current }]}
          onRowMenu={(row) => {
            void perform({ success: "已注销", failure: "注销失败", invalidate: [["me", "sessions"]], run: () => api(`/me/sessions/${row.key}`, { method: "DELETE" }) });
          }}
        />
      )}
    </AsyncView>
  );
}

function Tokens() {
  const query = useTokens();
  const { perform, busy } = useAction();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [scopes, setScopes] = useState<string[]>(["read"]);
  const [ttl, setTtl] = useState(90);
  const [created, setCreated] = useState<string | null>(null);
  const [revoke, setRevoke] = useState<TokenInfo | null>(null);
  const close = () => {
    setOpen(false);
    setCreated(null);
    setName("");
    setScopes(["read"]);
  };
  const createButton = (
    <Button
      size="sm"
      variant="primary"
      onClick={() => {
        setOpen(true);
      }}
    >
      创建 Token
    </Button>
  );
  return (
    <>
      <AsyncView query={query} isEmpty={(rows) => rows.length === 0} empty={{ title: "还没有 API Token", description: "Token 用于脚本访问管理 API。", action: createButton }}>
        {(rows) => (
          <DataTable<TokenInfo>
            id="tokens"
            label="API Token"
            rows={rows}
            rowKey={(row) => String(row.id)}
            primaryAction={createButton}
            columns={[
              { id: "name", header: "名称", cell: (row) => row.name, hideable: false },
              { id: "prefix", header: "前缀", cell: (row) => row.prefix ?? "—", data: true },
              { id: "scopes", header: "作用域", cell: (row) => row.scopes.join(", ") },
              { id: "expires", header: "过期时间", cell: (row) => fmtTime(row.expires_at), sortValue: (row) => row.expires_at, data: true },
            ]}
            rowMenu={() => [{ kind: "item", value: "revoke", label: "吊销", danger: true }]}
            onRowMenu={(row) => {
              setRevoke(row);
            }}
          />
        )}
      </AsyncView>
      <Dialog
        open={open}
        onOpenChange={(value) => {
          if (!value) close();
        }}
        title="创建 API Token"
        footer={
          created === null ? (
            <Button
              variant="primary"
              disabled={busy || name.trim() === "" || scopes.length === 0}
              onClick={() => {
                void perform({ success: "已创建", failure: "创建失败", invalidate: [["me", "tokens"]], run: () => api<{ token: string }>("/me/tokens", { method: "POST", body: { name: name.trim(), scopes, ttl_days: ttl } }) }).then((reply) => {
                  if (reply !== undefined) setCreated(reply.token);
                });
              }}
            >
              创建
            </Button>
          ) : (
            <Button variant="primary" onClick={close}>
              完成
            </Button>
          )
        }
      >
        {created === null ? (
          <div className="page-stack">
            <Field label="名称" required>
              <Input
                value={name}
                onChange={(event) => {
                  setName(event.target.value);
                }}
              />
            </Field>
            <div className="row-gap" role="group" aria-label="作用域">
              {SCOPES.map((scope) => (
                <Checkbox
                  key={scope}
                  label={scope}
                  checked={scopes.includes(scope)}
                  onChange={(checked) => {
                    setScopes(checked ? [...scopes, scope] : scopes.filter((item) => item !== scope));
                  }}
                />
              ))}
            </div>
            <NumberField label="有效期" unit="天" min={1} max={365} value={ttl} onChange={setTtl} />
          </div>
        ) : (
          <div className="page-stack">
            <Alert tone="warn" title="只显示这一次">关闭后无法再次查看，请立即复制保存。</Alert>
            <div className="row-gap">
              <code className="mono token-once">{created}</code>
              <CopyButton value={created} label="复制 Token" />
            </div>
          </div>
        )}
      </Dialog>
      <AlertDialog
        open={revoke !== null}
        onOpenChange={(value) => {
          if (!value) setRevoke(null);
        }}
        title={`吊销 ${revoke?.name ?? ""}`}
        description="吊销后使用该 Token 的脚本立即失效。"
        confirmLabel="吊销"
        busy={busy}
        onConfirm={() => {
          if (revoke === null) return;
          void perform({ success: "已吊销", failure: "吊销失败", invalidate: [["me", "tokens"]], run: () => api(`/me/tokens/${String(revoke.id)}`, { method: "DELETE" }) }).then(() => {
            setRevoke(null);
          });
        }}
      />
    </>
  );
}

function Users() {
  const query = useUsers(true);
  const { perform, busy } = useAction();
  const [open, setOpen] = useState(false);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [role, setRole] = useState("operator");
  const createButton = (
    <Button
      size="sm"
      variant="primary"
      onClick={() => {
        setOpen(true);
      }}
    >
      新建用户
    </Button>
  );
  return (
    <>
      <AsyncView query={query}>
        {(rows) => (
          <DataTable<UserSummary>
            id="users"
            label="用户"
            rows={rows}
            rowKey={(row) => String(row.id)}
            primaryAction={createButton}
            searchText={(row) => row.username}
            columns={[
              { id: "name", header: "用户名", cell: (row) => row.username, sortValue: (row) => row.username, hideable: false },
              { id: "role", header: "角色", cell: (row) => ROLES.find((item) => item.value === row.role)?.label ?? row.role },
              { id: "state", header: "状态", cell: (row) => (row.disabled ? <Badge tone="danger">已禁用</Badge> : row.must_change_password ? <Badge tone="warn">待改密</Badge> : <Badge tone="success">正常</Badge>) },
              { id: "created", header: "创建时间", cell: (row) => fmtTime(row.created_at), sortValue: (row) => row.created_at, data: true },
            ]}
            rowMenu={(row) => [
              ...ROLES.filter((item) => item.value !== row.role).map((item) => ({ kind: "item" as const, value: `role:${item.value}`, label: `设为${item.label}` })),
              { kind: "divider" as const, value: "d" },
              { kind: "item" as const, value: row.disabled ? "enable" : "disable", label: row.disabled ? "启用" : "禁用", danger: !row.disabled },
            ]}
            onRowMenu={(row, value) => {
              const body = value.startsWith("role:") ? { role: value.slice(5) } : { disabled: value === "disable" };
              void perform({ success: "已更新", failure: "更新失败", invalidate: [["users"]], run: () => api(`/users/${String(row.id)}`, { method: "PATCH", body }) });
            }}
          />
        )}
      </AsyncView>
      <Dialog
        open={open}
        onOpenChange={setOpen}
        title="新建用户"
        footer={
          <Button
            variant="primary"
            disabled={busy || username.trim() === "" || password.length < 12}
            onClick={() => {
              void perform({ success: "已创建", failure: "创建失败", invalidate: [["users"]], run: () => api("/users", { method: "POST", body: { username: username.trim(), password, role } }) }).then((reply) => {
                if (reply === undefined) return;
                setOpen(false);
                setUsername("");
                setPassword("");
              });
            }}
          >
            创建
          </Button>
        }
      >
        <div className="page-stack">
          <Field label="用户名" required>
            <Input
              value={username}
              autoComplete="off"
              onChange={(event) => {
                setUsername(event.target.value);
              }}
            />
          </Field>
          <Field label="初始密码" required hint="至少 12 位；首次登录须修改">
            <Input
              type="password"
              value={password}
              autoComplete="new-password"
              onChange={(event) => {
                setPassword(event.target.value);
              }}
            />
          </Field>
          <Select label="角色" value={role} onChange={setRole} options={ROLES} />
        </div>
      </Dialog>
    </>
  );
}

const TIMEOUTS = [
  { key: "arm_secs", label: "等待引导（Armed）", fallback: 7200 },
  { key: "install_secs", label: "安装（Installing）", fallback: 5400 },
  { key: "firstboot_secs", label: "首次启动（Installed）", fallback: 1800 },
  { key: "configure_secs", label: "初始化（Configuring）", fallback: 3600 },
] as const;

function Timeouts() {
  const state = useSingleton("timeouts");
  const doc = state.doc ?? Object.fromEntries(TIMEOUTS.map((item) => [item.key, item.fallback]));
  return (
    <Card>
      <div className="page-stack">
        <CardLabel>装机超时</CardLabel>
        <div className="field-grid">
          {TIMEOUTS.map((item) => {
            const raw = doc[item.key];
            const secs = typeof raw === "number" ? raw : item.fallback;
            return (
              <NumberField
                key={item.key}
                label={item.label}
                unit="分钟"
                min={1}
                max={1440}
                value={Math.round(secs / 60)}
                onChange={(value) => {
                  state.setDoc({ ...doc, [item.key]: value * 60 });
                }}
              />
            );
          })}
        </div>
        <div className="row-end">
          <SingletonSave kind="timeouts" state={state} fallback={doc} />
        </div>
      </div>
    </Card>
  );
}

const SECRET = /(_file|password|token|key)$/u;

function StartupConfig({ info }: { info: SystemInfo }) {
  const entries = Object.entries(info).filter(([key]) => key !== "version");
  return (
    <Card>
      <CardLabel>启动配置（只读）</CardLabel>
      <p className="muted-text">需修改启动配置（环境变量）后重启 xpxe-control 才会生效。</p>
      <dl className="kv">
        {entries.map(([key, value]) => (
          <div key={key} className="kv-row">
            <dt className="mono">{key}</dt>
            <dd className="mono">{SECRET.test(key) && key !== "tls_cert_file" ? (value === null ? "未配置" : "已配置") : value === null ? "—" : Array.isArray(value) ? value.join(", ") : typeof value === "object" ? JSON.stringify(value) : String(value as string | number | boolean)}</dd>
          </div>
        ))}
      </dl>
    </Card>
  );
}

function Account({ me }: { me: Me }) {
  const system = useSystemInfo();
  const { setEditorOpen } = useTheme();
  const admin = me.role === "admin";
  return (
    <div className="page-stack">
      <Section title="我的账户" description={`${me.user.username} · ${ROLES.find((item) => item.value === me.role)?.label ?? me.role}`}>
        <Card>
          <CardLabel>修改密码</CardLabel>
          <PasswordForm />
        </Card>
        <Sessions />
        <Tokens />
      </Section>
      {admin && (
        <Section title="用户管理">
          <Users />
        </Section>
      )}
      <Section title="装机">
        <Timeouts />
      </Section>
      {system.data != null && (
        <Section title="系统">
          <StartupConfig info={system.data} />
        </Section>
      )}
      <Section title="外观">
        <Button
          variant="outline"
          onClick={() => {
            setEditorOpen(true);
          }}
        >
          打开主题编辑器
        </Button>
      </Section>
    </div>
  );
}

export function SettingsPage() {
  const me = useMe();
  return (
    <AsyncView query={me} isEmpty={(data) => data === null} empty={{ title: "未登录", description: "登录后显示账户信息。" }}>
      {(data) => (data === null ? null : <Account me={data} />)}
    </AsyncView>
  );
}
