// 参考实现：.ui-input、.ui-textarea、.input-group、.input-addon、.input-action-btn、.ui-input.has-error、.ui-error-msg、.form-field、.ui-label、.ui-hint。
// 基于 Ark Field：label/描述/错误与控件自动关联（aria-describedby、aria-invalid）。
import { Field as ArkField } from "@ark-ui/react/field";
import type { ComponentPropsWithRef, ReactNode } from "react";
import { cx } from "../../lib/cx";

export interface FieldProps {
  label: string;
  hint?: string;
  error?: string | undefined;
  required?: boolean;
  children: ReactNode;
  className?: string;
}

export function Field({ label, hint, error, required = false, children, className }: FieldProps) {
  return (
    <ArkField.Root invalid={error !== undefined} required={required} className={cx("form-field", className)}>
      <ArkField.Label className="ui-label">
        {label}
        {required && <span aria-hidden="true"> *</span>}
      </ArkField.Label>
      {children}
      {hint !== undefined && error === undefined && <ArkField.HelperText className="ui-hint">{hint}</ArkField.HelperText>}
      <FieldError>{error}</FieldError>
    </ArkField.Root>
  );
}

export function FieldError({ children }: { children?: ReactNode }) {
  return <ArkField.ErrorText className="ui-error-msg">{children}</ArkField.ErrorText>;
}

export function Input({ className, ...rest }: ComponentPropsWithRef<"input">) {
  return <ArkField.Input className={cx("ui-input", className)} {...rest} />;
}

export function Textarea({ className, ...rest }: ComponentPropsWithRef<"textarea">) {
  return <ArkField.Textarea className={cx("ui-textarea", className)} {...rest} />;
}

export function InputGroup({ addon, action, children }: { addon?: ReactNode; action?: ReactNode; children: ReactNode }) {
  return (
    <div className="input-group">
      {addon !== undefined && <span className="input-addon">{addon}</span>}
      {children}
      {action}
    </div>
  );
}
