import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import { RouterProvider } from "react-router";
import { createRouter } from "./app/routes";
import { Toaster } from "./components/ui/toast";
import { ThemeProvider } from "./theme/ThemeProvider";
import { ApiError } from "./lib/api";

export function makeQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 10_000,
        refetchOnWindowFocus: false,
        // 4xx 不重试（401/403/404 重试无意义），网络与 5xx 重试一次
        retry: (failures, error) => failures < 1 && !(error instanceof ApiError && error.status >= 400 && error.status < 500),
      },
    },
  });
}

export function App() {
  const [client] = useState(makeQueryClient);
  const [router] = useState(createRouter);
  return (
    <QueryClientProvider client={client}>
      <ThemeProvider>
        <RouterProvider router={router} />
        <Toaster />
      </ThemeProvider>
    </QueryClientProvider>
  );
}
