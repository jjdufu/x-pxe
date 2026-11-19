// 参考实现：index.html #demoModal / #alertDialogModal / #demoDrawer、.modal-backdrop、.ui-dialog-header/body/footer、.ui-drawer；
// app.js openModal（打开后聚焦第一个输入）。焦点陷阱、ESC、外部点击由 Ark Dialog 状态机处理。
import { Dialog as ArkDialog } from "@ark-ui/react/dialog";
import { Portal } from "@ark-ui/react/portal";
import { X } from "lucide-react";
import { useRef, type ReactNode } from "react";
import { Button } from "./button";
import { InlineLayer } from "./layer";

interface BaseProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string | undefined;
  children?: ReactNode;
  footer?: ReactNode;
}

interface DialogProps extends BaseProps {
  /** 为 false 时不可关闭（无关闭按钮，ESC 与遮罩点击无效），如强制改密。 */
  closable?: boolean;
}

function Header({ title, description, closable }: { title: string; description?: string | undefined; closable: boolean }) {
  return (
    <div className="ui-dialog-header">
      <div>
        <ArkDialog.Title asChild>
          <h3>{title}</h3>
        </ArkDialog.Title>
        {description !== undefined && <ArkDialog.Description className="muted-text">{description}</ArkDialog.Description>}
      </div>
      {closable && (
        <ArkDialog.CloseTrigger className="icon-btn" aria-label="关闭">
          <X size={16} aria-hidden="true" />
        </ArkDialog.CloseTrigger>
      )}
    </div>
  );
}

/** 打开后聚焦的元素：内容区第一个输入（参考实现 openModal），没有输入时交给 Ark 默认（第一个可聚焦元素）。 */
function firstInput(container: HTMLElement | null): HTMLElement | null {
  return container?.querySelector<HTMLElement>("input:not([type=hidden]):not([disabled]), textarea, [role=combobox]") ?? null;
}

/** 对话框：打开后聚焦内容中第一个输入。 */
export function Dialog({ open, onOpenChange, title, description, children, footer, closable = true }: DialogProps) {
  const bodyRef = useRef<HTMLDivElement>(null);
  return (
    <ArkDialog.Root
      initialFocusEl={() => firstInput(bodyRef.current)}
      closeOnEscape={closable}
      closeOnInteractOutside={closable}
      open={open}
      onOpenChange={(details) => {
        onOpenChange(details.open);
      }}
    >
      <Portal>
        <ArkDialog.Backdrop className="ui-backdrop" />
        <ArkDialog.Positioner className="dialog-positioner">
          <ArkDialog.Content className="ui-dialog">
            <Header title={title} description={description} closable={closable} />
            <div className="ui-dialog-body" ref={bodyRef}>
              <InlineLayer.Provider value>{children}</InlineLayer.Provider>
            </div>
            {footer !== undefined && <div className="ui-dialog-footer">{footer}</div>}
          </ArkDialog.Content>
        </ArkDialog.Positioner>
      </Portal>
    </ArkDialog.Root>
  );
}

export interface AlertDialogProps extends Omit<BaseProps, "footer"> {
  confirmLabel: string;
  onConfirm: () => void;
  cancelLabel?: string;
  busy?: boolean;
  /** 确认条件未满足（如重新装机未输入主机名）。 */
  confirmDisabled?: boolean;
}

/** 危险确认（替代 confirm()）：role=alertdialog，点击遮罩不关闭，初始焦点在“取消”。 */
export function AlertDialog({ open, onOpenChange, title, description, children, confirmLabel, onConfirm, cancelLabel = "取消", busy = false, confirmDisabled = false }: AlertDialogProps) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <ArkDialog.Root
      role="alertdialog"
      initialFocusEl={() => cancelRef.current}
      closeOnInteractOutside={false}
      open={open}
      onOpenChange={(details) => {
        onOpenChange(details.open);
      }}
    >
      <Portal>
        <ArkDialog.Backdrop className="ui-backdrop" />
        <ArkDialog.Positioner className="dialog-positioner">
          <ArkDialog.Content className="ui-dialog">
            <Header title={title} description={description} closable={false} />
            {children !== undefined && <div className="ui-dialog-body">
              <InlineLayer.Provider value>{children}</InlineLayer.Provider>
            </div>}
            <div className="ui-dialog-footer">
              <ArkDialog.CloseTrigger asChild>
                <Button ref={cancelRef} variant="outline">
                  {cancelLabel}
                </Button>
              </ArkDialog.CloseTrigger>
              <Button variant="danger" disabled={busy || confirmDisabled} onClick={onConfirm}>
                {confirmLabel}
              </Button>
            </div>
          </ArkDialog.Content>
        </ArkDialog.Positioner>
      </Portal>
    </ArkDialog.Root>
  );
}

/** 侧边抽屉（节点详情、Theme Editor）。 */
export function Drawer({ open, onOpenChange, title, description, children, footer }: BaseProps) {
  return (
    <ArkDialog.Root
      open={open}
      onOpenChange={(details) => {
        onOpenChange(details.open);
      }}
    >
      <Portal>
        <ArkDialog.Backdrop className="ui-backdrop" />
        <ArkDialog.Positioner className="dialog-positioner drawer-positioner">
          <ArkDialog.Content className="ui-drawer">
            <Header title={title} description={description} closable />
            <div className="ui-drawer-body">
              <InlineLayer.Provider value>{children}</InlineLayer.Provider>
            </div>
            {footer !== undefined && <div className="ui-dialog-footer">{footer}</div>}
          </ArkDialog.Content>
        </ArkDialog.Positioner>
      </Portal>
    </ArkDialog.Root>
  );
}
