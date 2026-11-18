import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { ApiError } from "../lib/api";
import { AsyncView } from "./AsyncView";

function View({ fetcher }: { fetcher: () => Promise<string[]> }) {
  const query = useQuery({ queryKey: ["items"], queryFn: fetcher, retry: false });
  return (
    <AsyncView query={query} isEmpty={(items) => items.length === 0} empty={{ title: "空", description: "没有内容" }}>
      {(items) => <ul>{items.map((item) => <li key={item}>{item}</li>)}</ul>}
    </AsyncView>
  );
}

const mount = (fetcher: () => Promise<string[]>) =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <View fetcher={fetcher} />
    </QueryClientProvider>,
  );

describe("AsyncView 四态", () => {
  it("Loading → Populated", async () => {
    mount(() => Promise.resolve(["a"]));
    expect(screen.getByLabelText("加载中")).toBeTruthy();
    expect(await screen.findByText("a")).toBeTruthy();
  });

  it("Empty", async () => {
    mount(() => Promise.resolve([]));
    expect(await screen.findByText("空")).toBeTruthy();
  });

  it("Error 显示错误码，“重新加载”可恢复", async () => {
    let fail = true;
    mount(() => (fail ? Promise.reject(new ApiError(500, "INTERNAL", "炸了")) : Promise.resolve(["ok"])));
    expect(await screen.findByText("INTERNAL")).toBeTruthy();
    expect(screen.getByText("炸了")).toBeTruthy();
    fail = false;
    await userEvent.click(screen.getByRole("button", { name: "重新加载" }));
    expect(await screen.findByText("ok")).toBeTruthy();
  });
});
