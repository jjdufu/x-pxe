// 审计日志（step-12 §3.9）：服务端分页与筛选（操作者、动作、结果、时间范围）、哈希链校验、导出 JSON Lines。
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { DataTable } from "../../../components/ui/data-table";
import { Alert, Badge } from "../../../components/ui/feedback";
import { Field, Input } from "../../../components/ui/field";
import { Section } from "../../../components/ui/layout";
import { Pagination } from "../../../components/ui/pagination";
import { Select } from "../../../components/ui/select";
import { Segmented } from "../../../components/ui/tabs";
import { api } from "../../../lib/api";
import { fmtTime } from "../../../lib/format";
import type { AuditEntry, AuditPage as AuditPageData } from "../../../lib/types";
import { AsyncView } from "../../AsyncView";
import { errorText, useAudit } from "../../data";

const PAGE_SIZE = 50;
const ALL = "__all__";

interface VerifyResult {
  checked: number;
  first_broken: number | null;
}

function toUnix(value: string, end: boolean): string {
  if (value === "") return "";
  const time = Date.parse(`${value}T${end ? "23:59:59" : "00:00:00"}`);
  return Number.isNaN(time) ? "" : String(Math.floor(time / 1000));
}

function atText(at: string | number): string {
  return typeof at === "number" ? fmtTime(at) : fmtTime(Math.floor(Date.parse(at) / 1000));
}

export function AuditPage() {
  const [actor, setActor] = useState(ALL);
  const [action, setAction] = useState(ALL);
  const [outcome, setOutcome] = useState(ALL);
  const [since, setSince] = useState("");
  const [until, setUntil] = useState("");
  const [page, setPage] = useState(1);
  const params = new URLSearchParams({ page: String(page), page_size: String(PAGE_SIZE) });
  if (actor !== ALL) params.set("actor", actor);
  if (action !== ALL) params.set("action", action);
  if (outcome !== ALL) params.set("outcome", outcome);
  const from = toUnix(since, false);
  const to = toUnix(until, true);
  if (from !== "") params.set("since", from);
  if (to !== "") params.set("until", to);
  const query = useAudit(params.toString());
  const verify = useQuery({ queryKey: ["audit", "verify"], queryFn: () => api<VerifyResult | null>("/audit/verify"), enabled: false });
  const reset = (setter: (value: string) => void) => (value: string) => {
    setter(value);
    setPage(1);
  };
  const exportHref = `/api/v1/audit/export${from === "" ? "" : `?since=${from}`}`;
  return (
    <Section
      title="审计日志"
      description="所有写操作与登录记录；记录以哈希链串联，可校验是否被篡改。"
      actions={
        <div className="row-gap">
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              void verify.refetch();
            }}
          >
            校验哈希链
          </Button>
          <a className="ui-btn ui-btn-outline ui-btn-sm" href={exportHref} download="xpxe-audit.jsonl">
            导出 JSON Lines
          </a>
        </div>
      }
    >
      {verify.data != null &&
        (verify.data.first_broken === null ? (
          <Alert tone="success" title="哈希链完整">
            已校验 {verify.data.checked} 条记录。
          </Alert>
        ) : (
          <Alert tone="danger" title="哈希链断裂">
            第一处断链位于记录 #{verify.data.first_broken}（已校验 {verify.data.checked} 条）。
          </Alert>
        ))}
      {verify.error !== null && <Alert tone="danger" title="校验失败">{errorText(verify.error)}</Alert>}
      <div className="field-grid audit-filters">
        <Select label="操作者" value={actor} onChange={reset(setActor)} options={[{ value: ALL, label: "全部" }, ...(query.data?.actors ?? []).map((item) => ({ value: item, label: item }))]} />
        <Select label="动作" value={action} onChange={reset(setAction)} options={[{ value: ALL, label: "全部" }, ...(query.data?.actions ?? []).map((item) => ({ value: item, label: item }))]} />
        <Segmented label="结果" value={outcome} onChange={reset(setOutcome)} options={[{ value: ALL, label: "全部" }, { value: "ok", label: "成功" }, { value: "denied", label: "拒绝" }, { value: "failed", label: "失败" }]} />
        <Field label="起始日期">
          <Input type="date" value={since} onChange={(event) => { reset(setSince)(event.target.value); }} />
        </Field>
        <Field label="截止日期">
          <Input type="date" value={until} onChange={(event) => { reset(setUntil)(event.target.value); }} />
        </Field>
      </div>
      <AsyncView<AuditPageData | null> query={query} isEmpty={(data) => data === null || data.entries.length === 0} empty={{ title: "没有匹配的审计记录", description: "调整筛选条件后重试。" }}>
        {(data) =>
          data === null ? null : (
            <div className="page-stack">
              <DataTable<AuditEntry>
                id="audit"
                label="审计日志"
                rows={data.entries}
                rowKey={(row) => String(row.id)}
                columns={[
                  { id: "at", header: "时间", cell: (row) => atText(row.at), data: true },
                  { id: "actor", header: "操作者", cell: (row) => row.actor, hideable: false },
                  { id: "ip", header: "来源 IP", cell: (row) => row.ip, data: true },
                  { id: "action", header: "动作", cell: (row) => <span className="mono">{row.action}</span> },
                  { id: "target", header: "对象", cell: (row) => <span className="mono">{row.target}</span> },
                  { id: "outcome", header: "结果", cell: (row) => <Badge tone={row.outcome === "ok" ? "success" : "danger"}>{row.outcome}</Badge> },
                ]}
              />
              <Pagination count={data.total} pageSize={PAGE_SIZE} page={page} onPageChange={setPage} />
            </div>
          )
        }
      </AsyncView>
    </Section>
  );
}
