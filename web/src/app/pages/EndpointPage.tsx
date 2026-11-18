// Step 11 占位页：真实数据经 AsyncView 走完四态；业务内容在 Step 12 实现。
import { useQuery } from "@tanstack/react-query";
import { CodeBlock } from "../../components/ui/code";
import { Card, Section } from "../../components/ui/layout";
import { api } from "../../lib/api";
import { AsyncView } from "../AsyncView";

export interface EndpointPageProps {
  title: string;
  description: string;
  endpoint: string;
  queryKey: readonly string[];
  emptyTitle: string;
  emptyDescription: string;
}

function isEmptyValue(value: unknown): boolean {
  return value === null || (Array.isArray(value) && value.length === 0);
}

export function EndpointPage({ title, description, endpoint, queryKey, emptyTitle, emptyDescription }: EndpointPageProps) {
  const query = useQuery({ queryKey: [...queryKey], queryFn: () => api<unknown>(endpoint) });
  return (
    <Section title={title} description={description}>
      <AsyncView query={query} isEmpty={isEmptyValue} empty={{ title: emptyTitle, description: emptyDescription }}>
        {(data) => (
          <Card>
            <p className="muted-text">数据来自 GET /api/v1{endpoint}。完整页面在 Step 12 实现。</p>
            <CodeBlock label={`${title} 数据`} code={JSON.stringify(data, null, 2)} />
          </Card>
        )}
      </AsyncView>
    </Section>
  );
}
