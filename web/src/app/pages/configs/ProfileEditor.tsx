// Profile 编辑器（step-12 §3.4）：左表单（Accordion 分组）+ 右侧实时预览；300 ms 去抖。
import { useState } from "react";
import { useParams } from "react-router";
import { Accordion } from "../../../components/ui/accordion";
import { Checkbox, NumberField, RadioCards, Switch } from "../../../components/ui/controls";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Field, Textarea } from "../../../components/ui/field";
import { Section } from "../../../components/ui/layout";
import { Select } from "../../../components/ui/select";
import { Segmented } from "../../../components/ui/tabs";
import { AsyncView } from "../../AsyncView";
import { useConfigHead, useNodes } from "../../data";
import { LinesInput, ListInput, TextInput } from "./inputs";
import { PreviewPane } from "./PreviewPane";
import { getPath, IssuesAlert, issueMap, SaveBar, setPath, StaleBanner, useDraft, usePreview, type Doc } from "./shared";

const COMPONENTS = ["main", "contrib", "non_free", "non_free_firmware"] as const;

function JsonField({ label, value, onChange, path, error }: { label: string; value: unknown; onChange: (value: unknown) => void; path: string; error?: string | undefined }) {
  const [text, setText] = useState(() => JSON.stringify(value, null, 2));
  const [bad, setBad] = useState<string | null>(null);
  return (
    <div data-path={path}>
      <Field label={label} error={bad ?? error} hint="JSON">
        <Textarea
          className="mono"
          rows={8}
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            try {
              onChange(JSON.parse(event.target.value) as unknown);
              setBad(null);
            } catch {
              setBad("JSON 格式错误");
            }
          }}
        />
      </Field>
    </div>
  );
}

function ProfileForm({ doc, set, errors }: { doc: Doc; set: (path: string[], value: unknown) => void; errors: Record<string, string> }) {
  const str = (path: string[]) => {
    const value = getPath(doc, path);
    return typeof value === "string" ? value : "";
  };
  const arr = (path: string[]) => {
    const value = getPath(doc, path);
    return Array.isArray(value) ? value.map(String) : [];
  };
  const err = (path: string) => errors[path];
  const securityType = str(["security", "type"]) || "official";
  const partitioning = str(["partitioning", "type"]) || "atomic_lvm";
  const debugType = str(["debug", "type"]) || "off";
  const rootType = str(["accounts", "root", "type"]) || "disabled";
  const components = arr(["components", "items"]);
  const sizeMin = getPath(doc, ["disk", "size_gib", "min"]);
  const sizeMax = getPath(doc, ["disk", "size_gib", "max"]);
  const level = getPath(doc, ["debug", "level"]);
  const sections = [
    {
      value: "locale",
      title: "区域",
      content: (
        <div className="field-grid">
          <TextInput label="语言环境" path="/locale" value={str(["locale"])} onChange={(value) => { set(["locale"], value); }} error={err("/locale")} />
          <TextInput label="键盘布局" path="/keymap" value={str(["keymap"])} onChange={(value) => { set(["keymap"], value); }} error={err("/keymap")} />
          <TextInput label="时区" path="/timezone" value={str(["timezone"])} onChange={(value) => { set(["timezone"], value); }} error={err("/timezone")} />
        </div>
      ),
    },
    {
      value: "network",
      title: "网络与控制台",
      content: (
        <div className="field-grid">
          <ListInput label="NTP 服务器" path="/ntp" value={arr(["ntp"])} onChange={(value) => { set(["ntp"], value); }} error={err("/ntp")} />
          <ListInput label="控制台" path="/console/items" value={arr(["console", "items"])} onChange={(value) => { set(["console", "items"], value); }} error={err("/console/items")} />
        </div>
      ),
    },
    {
      value: "mirror",
      title: "镜像源与 APT",
      content: (
        <div className="page-stack">
          <Segmented label="协议" value={str(["mirror", "scheme"]) || "http"} onChange={(value) => { set(["mirror", "scheme"], value); }} options={[{ value: "http", label: "HTTP" }, { value: "https", label: "HTTPS" }]} />
          <div className="field-grid">
            <TextInput label="主机:端口" path="/mirror/host_port" value={str(["mirror", "host_port"])} onChange={(value) => { set(["mirror", "host_port"], value); }} error={err("/mirror/host_port")} />
            <TextInput label="目录" path="/mirror/directory" value={str(["mirror", "directory"])} onChange={(value) => { set(["mirror", "directory"], value); }} error={err("/mirror/directory")} />
            <TextInput label="HTTP 代理" path="/mirror/proxy" placeholder="不使用" value={str(["mirror", "proxy"])} onChange={(value) => { set(["mirror", "proxy"], value === "" ? null : value); }} error={err("/mirror/proxy")} />
          </div>
          <RadioCards
            label="安全更新源"
            value={securityType}
            onChange={(value) => {
              set(["security"], value === "mirror" ? { type: "mirror", host_port: str(["mirror", "host_port"]), directory: "/debian-security" } : { type: value });
            }}
            options={[
              { value: "official", title: "官方", sub: "security.debian.org" },
              { value: "mirror", title: "镜像", sub: "自建 security 镜像" },
              { value: "disabled", title: "关闭", sub: "离线环境" },
            ]}
          />
          {securityType === "mirror" && (
            <div className="field-grid">
              <TextInput label="安全源主机:端口" path="/security/host_port" value={str(["security", "host_port"])} onChange={(value) => { set(["security", "host_port"], value); }} error={err("/security/host_port")} />
              <TextInput label="安全源目录" path="/security/directory" value={str(["security", "directory"])} onChange={(value) => { set(["security", "directory"], value); }} error={err("/security/directory")} />
            </div>
          )}
          <div className="row-gap">
            <Switch label="updates" checked={getPath(doc, ["suites", "updates"]) === true} onChange={(value) => { set(["suites", "updates"], value); }} />
            <Switch label="backports" checked={getPath(doc, ["suites", "backports"]) === true} onChange={(value) => { set(["suites", "backports"], value); }} />
          </div>
          <div className="row-gap" data-path="/components/items">
            {COMPONENTS.map((item) => (
              <Checkbox
                key={item}
                label={item}
                checked={components.includes(item)}
                onChange={(checked) => {
                  set(["components", "items"], checked ? COMPONENTS.filter((name) => name === item || components.includes(name)) : components.filter((name) => name !== item));
                }}
              />
            ))}
          </div>
          <ListInput label="tasksel 任务" path="/tasks/items" value={arr(["tasks", "items"])} onChange={(value) => { set(["tasks", "items"], value); }} error={err("/tasks/items")} />
        </div>
      ),
    },
    {
      value: "accounts",
      title: "账户",
      content: (
        <div className="page-stack">
          <div className="row-gap">
            <span>root 登录</span>
            <Badge tone={rootType === "hash" ? "success" : "neutral"}>{rootType === "hash" ? "已设置口令哈希" : "禁用"}</Badge>
          </div>
          <JsonField label="账户（root / 主用户）" path="/accounts" value={getPath(doc, ["accounts"])} onChange={(value) => { set(["accounts"], value); }} error={err("/accounts")} />
        </div>
      ),
    },
    {
      value: "disk",
      title: "选盘规则",
      content: (
        <div className="page-stack">
          <RadioCards
            label="多块候选盘时"
            value={str(["disk", "prefer"]) || "unique"}
            onChange={(value) => { set(["disk", "prefer"], value); }}
            options={[
              { value: "unique", title: "唯一", sub: "多块候选即失败" },
              { value: "smallest", title: "最小", sub: "选容量最小" },
              { value: "largest", title: "最大", sub: "选容量最大" },
            ]}
          />
          <div className="field-grid">
            <NumberField label="最小容量" unit="GiB" min={1} max={65536} value={typeof sizeMin === "number" ? sizeMin : 16} onChange={(value) => { set(["disk", "size_gib", "min"], value); }} />
            <NumberField label="最大容量（0 = 不限）" unit="GiB" min={0} max={65536} value={typeof sizeMax === "number" ? sizeMax : 0} onChange={(value) => { set(["disk", "size_gib", "max"], value === 0 ? null : value); }} />
          </div>
          <LinesInput label="排除型号" path="/disk/exclude_models" value={arr(["disk", "exclude_models"])} onChange={(value) => { set(["disk", "exclude_models"], value); }} error={err("/disk/exclude_models")} />
        </div>
      ),
    },
    {
      value: "partitioning",
      title: "分区",
      content: (
        <div className="page-stack">
          <RadioCards
            label="分区方案"
            value={partitioning}
            onChange={(value) => { set(["partitioning"], value === "custom" ? { type: "custom", partitions: [] } : { type: value }); }}
            options={[
              { value: "atomic_lvm", title: "LVM", sub: "单分区 + LVM" },
              { value: "atomic_plain", title: "普通", sub: "单分区" },
              { value: "custom", title: "自定义", sub: "逐个分区" },
            ]}
          />
          {partitioning === "custom" && (
            <JsonField label="分区列表" path="/partitioning/partitions" value={getPath(doc, ["partitioning", "partitions"]) ?? []} onChange={(value) => { set(["partitioning", "partitions"], value); }} error={err("/partitioning/partitions")} />
          )}
        </div>
      ),
    },
    {
      value: "advanced",
      title: "高级",
      content: (
        <div className="page-stack">
          <Switch label="附加固件 cpio" checked={getPath(doc, ["firmware_cpio"]) === true} onChange={(value) => { set(["firmware_cpio"], value); }} />
          <Segmented label="调试" value={debugType} onChange={(value) => { set(["debug"], value === "debconf" ? { type: "debconf", level: 5 } : { type: "off" }); }} options={[{ value: "off", label: "关闭" }, { value: "debconf", label: "debconf" }]} />
          {debugType === "debconf" && <NumberField label="debconf 级别" min={1} max={5} value={typeof level === "number" ? level : 5} onChange={(value) => { set(["debug", "level"], value); }} />}
          <Select
            label="安装时升级"
            value={str(["pkgsel_upgrade"]) || "none"}
            onChange={(value) => { set(["pkgsel_upgrade"], value); }}
            options={[
              { value: "none", label: "不升级" },
              { value: "safe_upgrade", label: "安全升级" },
              { value: "full_upgrade", label: "完全升级" },
            ]}
          />
          <JsonField label="附加仓库" path="/extra_repos" value={getPath(doc, ["extra_repos"]) ?? []} onChange={(value) => { set(["extra_repos"], value); }} error={err("/extra_repos")} />
        </div>
      ),
    },
  ];
  return <Accordion sections={sections} defaultValue={["locale", "network", "mirror"]} />;
}

function Editor({ id }: { id: number }) {
  const head = useConfigHead<Doc>("profile", id);
  const nodes = useNodes();
  const draft = useDraft(head.data);
  const [context, setContext] = useState("dhcp");
  const live = usePreview("profile", draft.doc === null ? null : { document: draft.doc, context, base: { object_id: id, version: draft.base } });
  const issues = live.result?.fields ?? [];
  const errors = issueMap(issues);
  const contextOptions = [
    { value: "dhcp", label: "DHCP 样例" },
    { value: "static", label: "静态地址样例" },
    ...(nodes.data ?? []).filter((node) => node.profile_id === id).map((node) => ({ value: `node:${String(node.id)}`, label: `节点 ${node.hostname}` })),
  ];
  return (
    <AsyncView query={head}>
      {(data) => (
        <div className="page-stack">
          <StaleBanner show={draft.stale && draft.dirty} onReload={draft.reload} />
          <IssuesAlert issues={issues} />
          <div className="editor-2col">
            <div className="page-stack">
              {draft.doc !== null && (
                <ProfileForm
                  doc={draft.doc}
                  errors={errors}
                  set={(path, value) => {
                    if (draft.doc !== null) draft.setDoc(setPath(draft.doc, path, value));
                  }}
                />
              )}
            </div>
            <div className="sticky-preview page-stack">
              <Select label="预览上下文" value={context} onChange={setContext} options={contextOptions} />
              <PreviewPane label={`预览 · ${data.document.name as string}`} artifacts={live.result?.artifacts ?? []} error={live.error} loading={live.loading} />
              {(live.result?.warnings ?? []).length > 0 && (
                <Alert tone="info" title="提示">
                  {(live.result?.warnings ?? []).map((item) => item.message).join("；")}
                </Alert>
              )}
            </div>
          </div>
          <SaveBar kind="profile" objectId={id} draft={draft} diffPreview={live.result} valid={live.result?.ok ?? false} />
        </div>
      )}
    </AsyncView>
  );
}

export function ProfileEditorPage() {
  const params = useParams();
  const id = Number(params.id);
  return (
    <Section title="编辑 Profile" description="左侧修改，右侧 preseed / iPXE 实时预览；保存前可查看差异。">
      <Editor key={id} id={id} />
    </Section>
  );
}
