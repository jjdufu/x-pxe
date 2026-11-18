// 参考实现：.toast-container、.toast、.toast-close；app.js showToast（3.2 s 自动消失、可关闭、success/danger 变体、aria-live="polite"）。
import { Toast, Toaster as ArkToaster, createToaster } from "@ark-ui/react/toast";
import { X } from "lucide-react";
import { cx } from "../../lib/cx";

export const toaster = createToaster({ placement: "bottom-end", duration: 3200, gap: 8, overlap: false, max: 5 });

export type ToastVariant = "success" | "danger" | "info" | "warn";

/** 显示一条 Toast（替代 alert()）。 */
export function notify(title: string, variant: ToastVariant = "info", description?: string): void {
  toaster.create({ title, description, type: variant === "danger" ? "error" : variant === "warn" ? "warning" : variant });
}

const VARIANTS: Record<string, string> = { success: "toast-success", error: "toast-danger", warning: "toast-warn", info: "toast-info" };

export function Toaster() {
  return (
    <ArkToaster toaster={toaster}>
      {(toast) => (
        <Toast.Root className={cx("toast", VARIANTS[toast.type ?? "info"])}>
          <Toast.Title className="toast-title">{toast.title}</Toast.Title>
          {toast.description !== undefined && <Toast.Description className="toast-desc">{toast.description}</Toast.Description>}
          <Toast.CloseTrigger className="toast-close" aria-label="关闭通知">
            <X size={14} aria-hidden="true" />
          </Toast.CloseTrigger>
        </Toast.Root>
      )}
    </ArkToaster>
  );
}
