// 组件样例页（独立入口 gallery.html，不进入生产产物）：门禁第 7、8 项在此页上跑 axe 与键盘用例。
// URL 参数 ?mode=light|dark&gray=…&accent=…&radius=… 直接设定主题，便于矩阵测试。
import { useState } from "react";
import { Accordion } from "../components/ui/accordion";
import { Button, IconButton } from "../components/ui/button";
import { CodeBlock, CopyButton, DiffView } from "../components/ui/code";
import { MultiCombobox } from "../components/ui/combobox";
import { CommandPalette } from "../components/ui/command-palette";
import { Checkbox, NumberField, RadioCards, RangeSlider, Slider, Switch } from "../components/ui/controls";
import { DataTable, type Column } from "../components/ui/data-table";
import { AlertDialog, Dialog, Drawer } from "../components/ui/dialog";
import { Alert, Badge, Progress, Skeleton, Steps, Timeline, Tracker } from "../components/ui/feedback";
import { Field, Input, InputGroup, Textarea } from "../components/ui/field";
import { FileDrop } from "../components/ui/file-drop";
import { Card, Grid, Section } from "../components/ui/layout";
import { ContextMenu, Menu } from "../components/ui/menu";
import { HoverCard, Popover, Tooltip } from "../components/ui/overlay";
import { Pagination } from "../components/ui/pagination";
import { Select } from "../components/ui/select";
import { Segmented, Tabs, ToggleGroup } from "../components/ui/tabs";
import { notify } from "../components/ui/toast";
import { EmptyState, ErrorState, DefaultSkeleton } from "../app/AsyncView";
import { Copy, MoreHorizontal, Trash2 } from "lucide-react";

interface Row {
  id: string;
  host: string;
  mac: string;
  state: string;
}

const ROWS: Row[] = [
  { id: "1", host: "node-01", mac: "52:54:00:00:00:01", state: "Ready" },
  { id: "2", host: "node-02", mac: "52:54:00:00:00:02", state: "Installing" },
  { id: "3", host: "node-03", mac: "52:54:00:00:00:03", state: "Failed" },
  { id: "4", host: "edge-01", mac: "52:54:00:00:00:04", state: "Ready" },
];

const COLUMNS: Column<Row>[] = [
  { id: "host", header: "主机名", cell: (row) => row.host, sortValue: (row) => row.host, hideable: false },
  { id: "mac", header: "MAC", cell: (row) => row.mac, data: true },
  { id: "state", header: "状态", cell: (row) => <Badge tone={row.state === "Ready" ? "success" : row.state === "Failed" ? "danger" : "warn"}>{row.state}</Badge> },
];

const ZONES = [
  { value: "Asia/Shanghai", label: "Asia/Shanghai" },
  { value: "UTC", label: "UTC" },
  { value: "Europe/Berlin", label: "Europe/Berlin" },
  { value: "America/New_York", label: "America/New_York" },
];

export function Gallery() {
  const [select, setSelect] = useState<string | null>(null);
  const [zones, setZones] = useState<string[]>([]);
  const [dialog, setDialog] = useState(false);
  const [alert, setAlert] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const [palette, setPalette] = useState(false);
  const [lastAction, setLastAction] = useState("无");
  const [on, setOn] = useState(false);
  const [checked, setChecked] = useState(false);
  const [mode, setMode] = useState("uefi");
  const [tab, setTab] = useState("a");
  const [seg, setSeg] = useState("all");
  const [view, setView] = useState("table");
  const [timeout, setTimeoutValue] = useState(30);
  const [radius, setRadius] = useState([6]);
  const [range, setRange] = useState([100, 500]);
  const [page, setPage] = useState(1);

  return (
    <main className="canvas gallery" id="gallery">
      <Section title="组件样例" description="Step 11 组件库全集；门禁在此页上做对比度与键盘检查。">
        <p className="muted-text" data-testid="last-action">
          最近操作：{lastAction}
        </p>
      </Section>

      <Section title="按钮与徽章">
        <div className="row-gap">
          <Button variant="primary">主按钮</Button>
          <Button variant="secondary">次按钮</Button>
          <Button variant="outline">描边</Button>
          <Button variant="ghost">幽灵</Button>
          <Button variant="danger">危险</Button>
          <Button variant="primary" size="sm">
            小
          </Button>
          <Button disabled>禁用</Button>
          <IconButton label="复制">
            <Copy size={14} aria-hidden="true" />
          </IconButton>
          <Badge>默认</Badge>
          <Badge tone="success" dot>
            Ready
          </Badge>
          <Badge tone="warn">Installing</Badge>
          <Badge tone="danger">Failed</Badge>
          <Badge tone="outline">outline</Badge>
          <Badge tone="solid">solid</Badge>
        </div>
      </Section>

      <Section title="表单">
        <Grid cols={2}>
          <Card>
            <Field label="主机名" hint="小写字母、数字与连字符">
              <Input placeholder="node-01" />
            </Field>
            <Field label="IPv4" error="不是合法地址">
              <Input defaultValue="10.0.0.300" className="has-error" />
            </Field>
            <Field label="备注">
              <Textarea rows={2} />
            </Field>
            <Field label="镜像源">
              <InputGroup addon="http://" action={<CopyButton value="deb.debian.org" />}>
                <input aria-label="镜像源主机" defaultValue="deb.debian.org" />
              </InputGroup>
            </Field>
          </Card>
          <Card>
            <Select label="引导方式" options={[{ value: "uefi", label: "UEFI PXE" }, { value: "bios", label: "BIOS PXE" }, { value: "http", label: "UEFI HTTP Boot" }]} value={select} onChange={setSelect} />
            <div className="form-field">
              <MultiCombobox label="时区" options={ZONES} value={zones} onChange={setZones} />
            </div>
            <div className="flex flex-wrap items-center gap-4">
              <Switch checked={on} onChange={setOn} label="启用模块" />
              <Checkbox checked={checked} onChange={setChecked} label="同意" />
              <Checkbox checked="indeterminate" onChange={() => undefined} label="部分选中" />
            </div>
            <NumberField label="超时" unit="分钟" value={timeout} onChange={setTimeoutValue} min={1} max={240} />
          </Card>
        </Grid>
        <RadioCards
          label="引导网络模式"
          value={mode}
          onChange={setMode}
          options={[
            { value: "uefi", title: "ProxyDHCP", sub: "与现网 DHCP 并存" },
            { value: "bios", title: "复用 DHCP", sub: "next-server 指向本机" },
            { value: "off", title: "关闭", sub: "只用 Redfish" },
          ]}
        />
        <Grid cols={2}>
          <Slider label="圆角" min={0} max={99} value={radius} onChange={setRadius} />
          <RangeSlider label="选盘容量（GiB）" min={0} max={2000} step={10} value={range} onChange={setRange} />
        </Grid>
        <FileDrop
          label="上传固件"
          onFiles={(files) => {
            setLastAction(`选择文件 ${String(files.length)}`);
          }}
        />
      </Section>

      <Section title="浮层">
        <div className="row-gap">
          <Menu
            trigger={
              <Button variant="outline" data-testid="menu-trigger">
                <MoreHorizontal size={14} aria-hidden="true" />
                行操作
              </Button>
            }
            entries={[
              { kind: "item", value: "arm", label: "Arm", shortcut: "A" },
              { kind: "item", value: "disarm", label: "Disarm" },
              { kind: "divider", value: "d1" },
              { kind: "item", value: "delete", label: "删除", danger: true },
            ]}
            onSelect={(value) => {
              setLastAction(`菜单 ${value}`);
            }}
          />
          <Button
            variant="outline"
            onClick={() => {
              setDialog(true);
            }}
          >
            打开对话框
          </Button>
          <Button
            variant="danger"
            onClick={() => {
              setAlert(true);
            }}
          >
            <Trash2 size={14} aria-hidden="true" />
            删除
          </Button>
          <Button
            variant="outline"
            onClick={() => {
              setDrawer(true);
            }}
          >
            打开抽屉
          </Button>
          <Button
            variant="outline"
            onClick={() => {
              setPalette(true);
            }}
          >
            命令面板
          </Button>
          <Button
            variant="outline"
            onClick={() => {
              notify("已保存", "success", "Profile 版本 3");
            }}
          >
            Toast
          </Button>
          <Tooltip content="SHA-256 全文">
            <Button variant="ghost">Tooltip</Button>
          </Tooltip>
          <HoverCard content={<span className="mono">52:54:00:00:00:01</span>}>
            <Button variant="ghost">HoverCard</Button>
          </HoverCard>
          <Popover title="字段说明" content="超时后节点回到 Failed。">
            <Button variant="ghost">Popover</Button>
          </Popover>
        </div>
        <ContextMenu
          className="context-zone"
          entries={[
            { kind: "item", value: "copy", label: "复制 MAC", shortcut: "⌘C" },
            { kind: "item", value: "arm", label: "Arm" },
          ]}
          onSelect={(value) => {
            setLastAction(`右键 ${value}`);
          }}
        >
          <Card surface>在此区域右键</Card>
        </ContextMenu>
      </Section>

      <Section title="导航与结构">
        <Tabs
          label="预览"
          value={tab}
          onChange={setTab}
          items={[
            { value: "a", label: "preseed", content: <CodeBlock label="preseed" code={"d-i debian-installer/locale string en_US.UTF-8\nd-i netcfg/choose_interface select auto"} /> },
            { value: "b", label: "差异", content: <DiffView label="差异" lines={[{ kind: "same", text: "locale en_US" }, { kind: "del", text: "tz UTC" }, { kind: "add", text: "tz Asia/Shanghai" }]} /> },
          ]}
        />
        <div className="row-gap">
          <Segmented label="结果" value={seg} onChange={setSeg} options={[{ value: "all", label: "全部" }, { value: "ok", label: "成功" }, { value: "fail", label: "失败" }]} />
          <ToggleGroup label="视图" value={view} onChange={setView} options={[{ value: "table", label: "表格" }, { value: "card", label: "卡片" }]} />
        </div>
        <Accordion
          defaultValue={["disk"]}
          sections={[
            { value: "disk", title: "磁盘与分区", content: <p className="muted-text">确定性选盘。</p> },
            { value: "net", title: "网络", content: <p className="muted-text">静态或 DHCP。</p> },
          ]}
        />
        <Steps labels={["获取", "校验", "金丝雀", "激活"]} current={2} />
        <Pagination count={95} pageSize={10} page={page} onPageChange={setPage} />
      </Section>

      <Section title="数据">
        <DataTable id="gallery" label="样例节点" rows={ROWS} rowKey={(row) => row.id} columns={COLUMNS} searchText={(row) => `${row.host} ${row.mac}`} filter={{ label: "状态", options: [{ value: "Ready", label: "Ready" }, { value: "Installing", label: "Installing" }, { value: "Failed", label: "Failed" }], match: (row, value) => row.state === value }} />
        <Grid cols={3}>
          <Card>
            <Progress label="下载" value={62} />
            <Tracker label="近 30 天装机结果" days={Array.from({ length: 30 }, (_, index) => ({ date: `2026-11-${String(index + 1).padStart(2, "0")}`, status: index % 11 === 5 ? ("down" as const) : index % 7 === 3 ? ("degraded" as const) : ("ok" as const), ratio: 1 }))} />
          </Card>
          <Card>
            <Timeline
              entries={[
                { id: "1", title: "installer.started", meta: "12:01:02", status: "info" },
                { id: "2", title: "installer.finished", meta: "12:09:40", status: "success" },
                { id: "3", title: "agent.failed", meta: "12:15:11", status: "danger" },
              ]}
            />
          </Card>
          <Card>
            <Skeleton width="60%" />
            <Skeleton width="90%" />
          </Card>
        </Grid>
      </Section>

      <Section title="四态与提示">
        <Grid cols={2}>
          <EmptyState title="还没有节点" description="登记第一台目标机后，它会出现在这里。" onReset={() => undefined} action={<Button variant="primary" size="sm">登记节点</Button>} />
          <DefaultSkeleton />
        </Grid>
        <ErrorState error={new Error("控制面不可达")} onRetry={() => undefined} />
        <Grid cols={2}>
          <Alert tone="info" title="信息">后台出现新版本。</Alert>
          <Alert tone="success" title="成功">哈希链校验通过。</Alert>
          <Alert tone="warn" title="警告">资产 ABI 未对齐。</Alert>
          <Alert tone="danger" title="错误">bootnet 冲突。</Alert>
        </Grid>
      </Section>

      <Dialog open={dialog} onOpenChange={setDialog} title="新建 Profile" description="填写名称后保存。" footer={<Button variant="primary" onClick={() => { setDialog(false); setLastAction("对话框保存"); }}>保存</Button>}>
        <Field label="名称">
          <Input data-testid="dialog-input" />
        </Field>
      </Dialog>
      <AlertDialog open={alert} onOpenChange={setAlert} title="删除节点？" description="此操作不可撤销。" confirmLabel="删除" onConfirm={() => { setAlert(false); setLastAction("确认删除"); }} />
      <Drawer open={drawer} onOpenChange={setDrawer} title="节点详情">
        <p className="muted-text">node-01</p>
        <Button variant="outline">抽屉内按钮</Button>
      </Drawer>
      <CommandPalette
        open={palette}
        onOpenChange={setPalette}
        commands={[
          { id: "nodes", label: "节点", group: "跳转页面", run: () => { setLastAction("命令 节点"); } },
          { id: "assets", label: "资产", group: "跳转页面", run: () => { setLastAction("命令 资产"); } },
          { id: "theme", label: "切换主题", group: "主题", run: () => { setLastAction("命令 切换主题"); } },
        ]}
      />
    </main>
  );
}
