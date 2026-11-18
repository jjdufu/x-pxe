// 参考实现：styles.css .ui-btn、.ui-btn-{primary,secondary,outline,ghost,danger}、-sm/-lg/-icon、.icon-btn；components.js“按钮变体与状态”。
import type { ComponentPropsWithRef } from "react";
import { cx } from "../../lib/cx";

export type ButtonVariant = "primary" | "secondary" | "outline" | "ghost" | "danger";

export interface ButtonProps extends ComponentPropsWithRef<"button"> {
  variant?: ButtonVariant;
  size?: "sm" | "lg";
}

export function Button({ variant = "secondary", size, className, type = "button", ...rest }: ButtonProps) {
  return <button type={type} className={cx("ui-btn", `ui-btn-${variant}`, size !== undefined && `ui-btn-${size}`, className)} {...rest} />;
}

export interface IconButtonProps extends ComponentPropsWithRef<"button"> {
  /** 无文字按钮必须有可读名称。 */
  label: string;
}

export function IconButton({ label, className, type = "button", ...rest }: IconButtonProps) {
  return <button type={type} aria-label={label} title={label} className={cx("icon-btn", className)} {...rest} />;
}
