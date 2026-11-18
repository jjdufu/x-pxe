// 四态容器（规范 §5）：Populated / Empty / Loading / Error。出处：components.js untitled 区块 mode === 'empty' | 'loading' | 'error'。
// 后台刷新只更新查询缓存：isFetching 时保持已渲染内容，不切回骨架，组件不重新挂载。
import type { UseQueryResult } from "@tanstack/react-query";
import { Inbox, RotateCw } from "lucide-react";
import type { ReactNode } from "react";
import { ApiError } from "../lib/api";
import { Alert } from "../components/ui/feedback";
import { Button } from "../components/ui/button";
import { Skeleton } from "../components/ui/feedback";

export interface EmptySpec {
  title: string;
  description: string;
  icon?: ReactNode;
  onReset?: () => void;
  action?: ReactNode;
}

export interface AsyncViewProps<T> {
  query: Pick<UseQueryResult<T>, "data" | "error" | "isPending" | "isError" | "refetch">;
  isEmpty?: (data: T) => boolean;
  empty?: EmptySpec;
  loading?: ReactNode;
  children: (data: T) => ReactNode;
}

export function DefaultSkeleton() {
  return (
    <div className="ui-card async-loading" role="status" aria-busy="true" aria-label="加载中">
      <Skeleton width="40%" height={18} />
      <Skeleton width="90%" />
      <Skeleton width="75%" />
      <Skeleton width="60%" />
    </div>
  );
}

export function EmptyState({ title, description, icon, onReset, action }: EmptySpec) {
  return (
    <div className="ui-card empty-state">
      <div className="empty-icon" aria-hidden="true">
        {icon ?? <Inbox size={20} />}
      </div>
      <h3 className="empty-title">{title}</h3>
      <p className="muted-text">{description}</p>
      <div className="row-gap empty-actions">
        {onReset !== undefined && (
          <Button variant="outline" size="sm" onClick={onReset}>
            重置筛选
          </Button>
        )}
        {action}
      </div>
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const code = error instanceof ApiError ? error.code : "ERROR";
  const message = error instanceof Error ? error.message : String(error);
  return (
    <Alert
      tone="danger"
      title={code}
      action={
        <Button variant="danger" size="sm" onClick={onRetry}>
          <RotateCw size={13} aria-hidden="true" />
          重新加载
        </Button>
      }
    >
      {message}
    </Alert>
  );
}

export function AsyncView<T>({ query, isEmpty, empty, loading, children }: AsyncViewProps<T>) {
  if (query.isPending) return <>{loading ?? <DefaultSkeleton />}</>;
  if (query.isError) {
    return (
      <ErrorState
        error={query.error}
        onRetry={() => {
          void query.refetch();
        }}
      />
    );
  }
  const data = query.data as T;
  if (isEmpty?.(data) === true) {
    return <EmptyState {...(empty ?? { title: "暂无数据", description: "这里还没有任何内容。" })} />;
  }
  return <>{children(data)}</>;
}
