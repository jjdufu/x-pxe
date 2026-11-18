// 参考实现：.ui-card、.ui-card-surface、.card-label、.grid-2/3/4、.row-between、.row-gap、.form-field、.ui-label、.muted-text、.showcase-section/.section-head。
import type { ComponentPropsWithRef, ReactNode } from "react";
import { cx } from "../../lib/cx";

export function Card({ surface = false, className, ...rest }: ComponentPropsWithRef<"div"> & { surface?: boolean }) {
  return <div className={cx("ui-card", surface && "ui-card-surface", className)} {...rest} />;
}

export function CardLabel({ className, ...rest }: ComponentPropsWithRef<"div">) {
  return <div className={cx("card-label", className)} {...rest} />;
}

export function Grid({ cols, className, ...rest }: ComponentPropsWithRef<"div"> & { cols: 2 | 3 | 4 }) {
  return <div className={cx(`grid-${String(cols)}`, className)} {...rest} />;
}

export function Section({ title, description, actions, children, id }: { title: string; description?: string; actions?: ReactNode; children: ReactNode; id?: string }) {
  return (
    <section className="showcase-section" id={id}>
      <div className="section-head row-between">
        <div>
          <h2 className="section-title">{title}</h2>
          {description !== undefined && <p className="section-desc">{description}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <kbd>{children}</kbd>;
}
