// 节点页对话框：新建节点、从观测导入、危险操作确认（arm / disarm / 重新装机 / Redfish 部署 / 批量）。
import { useState } from "react";
import { Button } from "../../../components/ui/button";
import { AlertDialog, Dialog } from "../../../components/ui/dialog";
import { Field, Input } from "../../../components/ui/field";
import { Select } from "../../../components/ui/select";
import { Segmented } from "../../../components/ui/tabs";
import { api } from "../../../lib/api";
import type { ConfigObject, NodeRow, NodeSpec, Observation } from "../../../lib/types";
import { useAction, useConfigs, useObservations } from "../../data";

export interface SpecDraft {
  hostname: string;
  mac: string;
  firmware: string;
  channel: string;
  bmc: string;
  network: string;
  address: string;
  gateway: string;
  dns: string;
  profileId: string;
  planId: string;
}

export function emptyDraft(mac = ""): SpecDraft {
  return { hostname: "", mac, firmware: "uefi", channel: "lan", bmc: "", network: "dhcp", address: "", gateway: "", dns: "", profileId: "", planId: "" };
}

export function draftOf(node: NodeRow): SpecDraft {
  const network = node.spec.network;
  return {
    hostname: node.hostname,
    mac: node.mac,
    firmware: node.firmware,
    channel: node.channel.type,
    bmc: node.channel.bmc ?? "",
    network: network.type,
    address: network.type === "static" ? network.address : "",
    gateway: network.type === "static" ? network.gateway : "",
    dns: network.type === "static" ? network.dns.join(", ") : "",
    profileId: String(node.profile_id),
    planId: String(node.init_plan_id),
  };
}

export function specOf(draft: SpecDraft): NodeSpec {
  return {
    hostname: draft.hostname.trim(),
    fqdn: null,
    mac: draft.mac.trim().toLowerCase(),
    os_family: "debian13",
    firmware: draft.firmware === "bios" ? "bios" : "uefi",
    channel: draft.channel === "redfish" ? { type: "redfish", bmc: draft.bmc.trim() } : { type: "lan" },
    network:
      draft.network === "static"
        ? { type: "static", address: draft.address.trim(), gateway: draft.gateway.trim(), dns: draft.dns.split(/[\s,]+/u).filter((item) => item !== "") }
        : { type: "dhcp" },
  };
}

const options = (items: ConfigObject[] | undefined) => (items ?? []).map((item) => ({ value: String(item.id), label: item.name }));

/** 规格字段（新建对话框与详情抽屉“规格”页共用）；hostname/MAC 在编辑时只读。 */
export function SpecFields({ draft, onChange, locked = false, errors = {} }: { draft: SpecDraft; onChange: (draft: SpecDraft) => void; locked?: boolean; errors?: Record<string, string> }) {
  const profiles = useConfigs("profile");
  const plans = useConfigs("init_plan");
  const set = (patch: Partial<SpecDraft>) => {
    onChange({ ...draft, ...patch });
  };
  return (
    <div className="page-stack">
      <div className="field-grid">
        <Field label="主机名" required error={errors["/spec/hostname"]}>
          <Input
            value={draft.hostname}
            disabled={locked}
            onChange={(event) => {
              set({ hostname: event.target.value });
            }}
          />
        </Field>
        <Field label="MAC" required error={errors["/spec/mac"]}>
          <Input
            className="mono"
            value={draft.mac}
            disabled={locked}
            placeholder="52:54:00:12:34:56"
            onChange={(event) => {
              set({ mac: event.target.value });
            }}
          />
        </Field>
      </div>
      <div className="field-grid">
        <Segmented
          label="固件"
          options={[
            { value: "uefi", label: "UEFI" },
            { value: "bios", label: "BIOS" },
          ]}
          value={draft.firmware}
          onChange={(value) => {
            set({ firmware: value });
          }}
        />
        <Segmented
          label="通道"
          options={[
            { value: "lan", label: "LAN" },
            { value: "redfish", label: "Redfish" },
          ]}
          value={draft.channel}
          onChange={(value) => {
            set({ channel: value });
          }}
        />
      </div>
      {draft.channel === "redfish" && (
        <Field label="BMC 名称" required>
          <Input
            value={draft.bmc}
            onChange={(event) => {
              set({ bmc: event.target.value });
            }}
          />
        </Field>
      )}
      <Segmented
        label="网络"
        options={[
          { value: "dhcp", label: "DHCP" },
          { value: "static", label: "静态" },
        ]}
        value={draft.network}
        onChange={(value) => {
          set({ network: value });
        }}
      />
      {draft.network === "static" && (
        <div className="field-grid">
          <Field label="地址（CIDR）" required>
            <Input
              value={draft.address}
              placeholder="10.77.0.16/24"
              onChange={(event) => {
                set({ address: event.target.value });
              }}
            />
          </Field>
          <Field label="网关" required>
            <Input
              value={draft.gateway}
              onChange={(event) => {
                set({ gateway: event.target.value });
              }}
            />
          </Field>
          <Field label="DNS（逗号分隔）">
            <Input
              value={draft.dns}
              onChange={(event) => {
                set({ dns: event.target.value });
              }}
            />
          </Field>
        </div>
      )}
      <div className="field-grid">
        <Select
          label="装机 Profile"
          options={options(profiles.data)}
          value={draft.profileId === "" ? null : draft.profileId}
          onChange={(value) => {
            set({ profileId: value });
          }}
        />
        <Select
          label="初始化计划"
          options={options(plans.data)}
          value={draft.planId === "" ? null : draft.planId}
          onChange={(value) => {
            set({ planId: value });
          }}
        />
      </div>
    </div>
  );
}

/** 422 字段错误 → 路径映射。 */
export function fieldErrors(error: unknown): Record<string, string> {
  const fields = (error as { fields?: unknown } | null)?.fields;
  if (!Array.isArray(fields)) return {};
  const out: Record<string, string> = {};
  for (const item of fields as { path?: string; message?: string }[]) {
    if (item.path !== undefined && item.message !== undefined) out[item.path] = item.message;
  }
  return out;
}

export function NewNodeDialog({ open, onOpenChange, initialMac = "" }: { open: boolean; onOpenChange: (open: boolean) => void; initialMac?: string }) {
  const [draft, setDraft] = useState<SpecDraft>(() => emptyDraft(initialMac));
  const [errors, setErrors] = useState<Record<string, string>>({});
  const { perform, busy } = useAction();
  const [seenMac, setSeenMac] = useState(initialMac);
  if (seenMac !== initialMac) {
    setSeenMac(initialMac);
    setDraft(emptyDraft(initialMac));
  }
  const submit = async () => {
    let failure: unknown = null;
    const created = await perform({
      success: "节点已登记",
      failure: "登记失败",
      invalidate: [["nodes"]],
      run: () =>
        api<NodeRow>("/nodes", { method: "POST", body: { spec: specOf(draft), profile_id: Number(draft.profileId), init_plan_id: Number(draft.planId) } }).catch((error: unknown) => {
          failure = error;
          throw error;
        }),
    });
    if (created === undefined) {
      setErrors(fieldErrors(failure));
      return;
    }
    setDraft(emptyDraft());
    setErrors({});
    onOpenChange(false);
  };
  return (
    <Dialog
      open={open}
      onOpenChange={onOpenChange}
      title="新建节点"
      description="登记一台目标机：主机名与 MAC 登记后不可修改。"
      footer={
        <>
          <Button
            variant="outline"
            onClick={() => {
              onOpenChange(false);
            }}
          >
            取消
          </Button>
          <Button
            variant="primary"
            disabled={busy || draft.hostname === "" || draft.mac === "" || draft.profileId === "" || draft.planId === ""}
            onClick={() => {
              void submit();
            }}
          >
            登记
          </Button>
        </>
      }
    >
      <SpecFields draft={draft} onChange={setDraft} errors={errors} />
    </Dialog>
  );
}

export function ImportDialog({ open, onOpenChange, known, onPick }: { open: boolean; onOpenChange: (open: boolean) => void; known: readonly string[]; onPick: (mac: string) => void }) {
  const observations = useObservations();
  const seen = new Map<string, Observation>();
  for (const item of observations.data ?? []) if (!known.includes(item.mac) && item.mac !== "" && !seen.has(item.mac)) seen.set(item.mac, item);
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title="从观测导入" description="bootnet 看到但尚未登记的 MAC。">
      {seen.size === 0 ? (
        <p className="muted-text">没有未登记的 MAC</p>
      ) : (
        <ul className="list-plain">
          {[...seen.values()].map((item) => (
            <li key={item.mac} className="row-between">
              <span className="mono tabular">{item.mac}</span>
              <span className="muted-text">{item.arch === "" ? item.kind : item.arch}</span>
              <Button
                size="sm"
                variant="outline"
                onClick={() => {
                  onPick(item.mac);
                }}
              >
                登记为节点
              </Button>
            </li>
          ))}
        </ul>
      )}
    </Dialog>
  );
}

export type Pending = { kind: "arm" | "disarm" | "rearm" | "deploy" | "eject" | "reset"; nodes: NodeRow[] } | null;

const TITLES: Record<NonNullable<Pending>["kind"], string> = {
  arm: "确认 arm",
  disarm: "确认 disarm",
  rearm: "重新装机",
  deploy: "Redfish 部署",
  eject: "弹出虚拟介质",
  reset: "重置为 Idle",
};

const PATHS: Record<NonNullable<Pending>["kind"], string> = {
  arm: "arm",
  disarm: "disarm",
  rearm: "rearm",
  deploy: "redfish/deploy",
  eject: "redfish/eject",
  reset: "reset",
};

/** 危险操作确认：批量时列出受影响主机名；重新装机须输入主机名。 */
export function ConfirmAction({ pending, onClose }: { pending: Pending; onClose: () => void }) {
  const [typed, setTyped] = useState("");
  const { perform, busy } = useAction();
  const kind = pending?.kind ?? "arm";
  const nodes = pending?.nodes ?? [];
  const single = nodes.length === 1 ? nodes[0] : undefined;
  const needsName = kind === "rearm";
  const confirm = async () => {
    for (const node of nodes) {
      await perform({
        success: `${node.hostname}：${TITLES[kind]}完成`,
        failure: `${node.hostname}：${TITLES[kind]}失败`,
        invalidate: [["nodes"], ["overview"]],
        run: () => api(`/nodes/${String(node.id)}/${PATHS[kind]}`, { method: "POST", body: needsName ? { confirmation: typed } : {} }),
      });
    }
    setTyped("");
    onClose();
  };
  return (
    <AlertDialog
      open={pending !== null}
      onOpenChange={(open) => {
        if (!open) {
          setTyped("");
          onClose();
        }
      }}
      title={TITLES[kind]}
      description={single === undefined ? `将对 ${String(nodes.length)} 个节点执行此操作。` : `目标：${single.hostname}（${single.mac}）`}
      confirmLabel={kind === "rearm" ? "重新装机" : TITLES[kind].replace("确认 ", "")}
      busy={busy}
      confirmDisabled={needsName && (single === undefined || typed !== single.hostname)}
      onConfirm={() => {
        void confirm();
      }}
    >
      {kind === "deploy" && <p className="muted-text">将强制重启目标服务器，并从虚拟介质引导安装程序。</p>}
      {nodes.length > 1 && (
        <ul className="list-plain">
          {nodes.map((node) => (
            <li key={node.id} className="mono">
              {node.hostname}
            </li>
          ))}
        </ul>
      )}
      {needsName && single !== undefined && (
        <Field label={`输入主机名 ${single.hostname} 以确认`} required>
          <Input
            value={typed}
            onChange={(event) => {
              setTyped(event.target.value);
            }}
          />
        </Field>
      )}
    </AlertDialog>
  );
}
