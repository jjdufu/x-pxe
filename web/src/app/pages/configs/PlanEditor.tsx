// 初始化计划编辑器（step-12 §3.5）：左模块列表 + 中模块表单 + 右预览（本模块脚本 / 整份计划 / 差异）。
import { useState } from "react";
import { useParams } from "react-router";
import { CodeBlock } from "../../../components/ui/code";
import { Checkbox, NumberField, Switch } from "../../../components/ui/controls";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Section } from "../../../components/ui/layout";
import { Select } from "../../../components/ui/select";
import { Tabs } from "../../../components/ui/tabs";
import { cx } from "../../../lib/cx";
import type { InitModuleInfo, InitModules, ModuleSlot } from "../../../lib/types";
import { AsyncView } from "../../AsyncView";
import { useConfigHead, useInitModules } from "../../data";
import { LinesInput } from "./inputs";
import { SchemaForm } from "./SchemaForm";
import { DiffContent, getPath, IssuesAlert, issueMap, SaveBar, setPath, StaleBanner, useDraft, usePreview, type Doc } from "./shared";

function slotOf(doc: Doc, info: InitModuleInfo): Required<ModuleSlot> {
  const own = getPath(doc, ["modules", info.id]) as ModuleSlot | undefined;
  const defaults = info.defaults ?? {};
  return {
    enabled: own?.enabled ?? defaults.enabled ?? true,
    continue_on_error: own?.continue_on_error ?? defaults.continue_on_error ?? false,
    timeout_s: own?.timeout_s ?? null,
    settings: own?.settings ?? defaults.settings ?? {},
  };
}

function PackagesForm({ settings, catalog, onChange, errors }: { settings: Record<string, unknown>; catalog: InitModules; onChange: (value: Record<string, unknown>) => void; errors: Record<string, string> }) {
  const groups = (settings.groups ?? {}) as Record<string, boolean>;
  const list = (key: string) => (Array.isArray(settings[key]) ? (settings[key] as unknown[]).map(String) : []);
  return (
    <div className="page-stack">
      <div className="group-grid" data-path="/modules/packages/settings/groups">
        {catalog.package_groups.map((group) => (
          <Checkbox
            key={group.id}
            label={`${group.id}（${String(group.count)}）`}
            checked={groups[group.id] ?? false}
            onChange={(checked) => {
              onChange({ ...settings, groups: { ...groups, [group.id]: checked } });
            }}
          />
        ))}
      </div>
      <LinesInput label="额外安装" path="/modules/packages/settings/extra" mono value={list("extra")} onChange={(value) => { onChange({ ...settings, extra: value }); }} error={errors["/modules/packages/settings/extra"]} />
      <LinesInput label="排除" path="/modules/packages/settings/exclude" mono value={list("exclude")} onChange={(value) => { onChange({ ...settings, exclude: value }); }} error={errors["/modules/packages/settings/exclude"]} />
      <Switch label="安装推荐包" checked={settings.install_recommends === true} onChange={(value) => { onChange({ ...settings, install_recommends: value }); }} />
      <Switch label="zram-tools" checked={settings.zram_tools === true} onChange={(value) => { onChange({ ...settings, zram_tools: value }); }} />
    </div>
  );
}

function Editor({ id, catalog }: { id: number; catalog: InitModules }) {
  const head = useConfigHead<Doc>("init_plan", id);
  const draft = useDraft(head.data);
  const [selected, setSelected] = useState(catalog.modules[0]?.id ?? "");
  const [tab, setTab] = useState("module");
  const live = usePreview("init_plan", draft.doc === null ? null : { document: draft.doc, base: { object_id: id, version: draft.base } });
  const issues = live.result?.fields ?? [];
  const errors = issueMap(issues);
  const info = catalog.modules.find((item) => item.id === selected) ?? catalog.modules[0];
  const doc = draft.doc;
  const update = (path: string[], value: unknown) => {
    if (doc !== null) draft.setDoc(setPath(doc, path, value));
  };
  const moduleScript = live.result?.modules?.find((item) => item.name === info?.id)?.text ?? "";
  const plan = typeof live.result?.plan === "string" ? live.result.plan : JSON.stringify(live.result?.plan ?? null, null, 2);
  const hasError = (moduleId: string) => issues.some((issue) => issue.path.startsWith(`/modules/${moduleId}/`));
  return (
    <AsyncView query={head}>
      {() =>
        doc === null || info === undefined ? null : (
          <div className="page-stack">
            <StaleBanner show={draft.stale && draft.dirty} onReload={draft.reload} />
            <IssuesAlert issues={issues} title="跨模块检查" />
            <div className="row-gap">
              <span className="muted-text">最终安装包数</span>
              <strong className="tabular" data-testid="package-count">
                {live.result?.package_count ?? "—"}
              </strong>
              <Switch label="成功后重启" checked={getPath(doc, ["finalize", "reboot"]) !== false} onChange={(value) => { update(["finalize", "reboot"], value); }} />
            </div>
            <div className="mobile-select">
              <Select label="模块" value={info.id} onChange={setSelected} options={catalog.modules.map((item) => ({ value: item.id, label: `${String(item.order)}. ${item.title}` }))} />
            </div>
            <div className="editor-3col">
              <nav className="module-list" aria-label="模块">
                {catalog.modules.map((item) => {
                  const slot = slotOf(doc, item);
                  return (
                    <div key={item.id} className={cx("module-item", item.id === info.id && "is-active")}>
                      <button
                        type="button"
                        className="module-pick"
                        aria-current={item.id === info.id}
                        onClick={() => {
                          setSelected(item.id);
                        }}
                      >
                        <span className="nav-num tabular">{item.order}</span>
                        <span>{item.title}</span>
                        <span className={cx("status-dot", hasError(item.id) ? "is-danger" : slot.enabled ? "is-success" : "is-muted")} aria-hidden="true" />
                      </button>
                      <Switch label={`启用 ${item.title}`} checked={slot.enabled} onChange={(value) => { update(["modules", item.id, "enabled"], value); }} />
                    </div>
                  );
                })}
              </nav>
              <div className="page-stack" data-testid="module-form">
                <div className="row-gap">
                  <h3 className="h3">
                    {info.order}. {info.title}
                  </h3>
                  <Badge tone={slotOf(doc, info).enabled ? "success" : "neutral"}>{slotOf(doc, info).enabled ? "启用" : "关闭"}</Badge>
                </div>
                <div className="field-grid">
                  <Switch label="失败后继续" checked={slotOf(doc, info).continue_on_error} onChange={(value) => { update(["modules", info.id, "continue_on_error"], value); }} />
                  <NumberField
                    label={`超时（0 = 默认 ${String(info.default_timeout_s)} 秒）`}
                    unit="秒"
                    min={0}
                    max={86400}
                    value={slotOf(doc, info).timeout_s ?? 0}
                    onChange={(value) => { update(["modules", info.id, "timeout_s"], value === 0 ? null : value); }}
                  />
                </div>
                {info.id === "packages" ? (
                  <PackagesForm settings={slotOf(doc, info).settings} catalog={catalog} errors={errors} onChange={(value) => { update(["modules", info.id, "settings"], value); }} />
                ) : (
                  <SchemaForm
                    key={info.id}
                    value={slotOf(doc, info).settings}
                    sample={info.defaults?.settings ?? null}
                    path={`/modules/${info.id}/settings`}
                    errors={errors}
                    onChange={(value) => { update(["modules", info.id, "settings"], value); }}
                  />
                )}
                {info.id === "custom_snippet" && <Alert tone="warn" title="自定义片段">片段以 root 执行，请确认内容可信。</Alert>}
              </div>
              <div className="sticky-preview">
                <Tabs
                  label="计划预览"
                  value={tab}
                  onChange={setTab}
                  items={[
                    { value: "module", label: "本模块脚本", content: <CodeBlock code={moduleScript} label={`${info.id} 脚本`} /> },
                    { value: "plan", label: "整份计划", content: <CodeBlock code={plan} label="plan.json" /> },
                    { value: "diff", label: "与头版本差异", content: <DiffContent preview={live.result} /> },
                  ]}
                />
                {live.error !== null && <Alert tone="warn" title="预览失败">{live.error}</Alert>}
              </div>
            </div>
            <SaveBar kind="init_plan" objectId={id} draft={draft} diffPreview={live.result} valid={(live.result?.modules ?? []).length > 0} />
          </div>
        )
      }
    </AsyncView>
  );
}

export function PlanEditorPage() {
  const params = useParams();
  const id = Number(params.id);
  const catalog = useInitModules();
  return (
    <Section title="编辑初始化计划" description="20 个模块按执行顺序排列；右侧是本模块脚本与整份计划。">
      <AsyncView query={catalog}>{(data) => <Editor key={id} id={id} catalog={data} />}</AsyncView>
    </Section>
  );
}
