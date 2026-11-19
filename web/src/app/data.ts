// 服务端数据：全部经 react-query；写操作统一 Toast 反馈（失败带错误码）并标记相关查询失效。
import { useMutation, useQuery, useQueryClient, type QueryKey } from "@tanstack/react-query";
import { useCallback } from "react";
import { notify } from "../components/ui/toast";
import { api, ApiError } from "../lib/api";
import type {
  AssetsView,
  BootMenuStatus,
  AuditPage,
  BootnetStatus,
  ConfigHead,
  ConfigObject,
  EventDetail,
  HistoryRow,
  InitModules,
  IsoList,
  IsoTree,
  Me,
  NodeRow,
  Observation,
  Overview,
  Presets,
  SessionInfo,
  Snippets,
  SystemInfo,
  TokenInfo,
  UserSummary,
} from "../lib/types";

export const useMe = () => useQuery({ queryKey: ["me"], queryFn: () => api<Me | null>("/me"), retry: false });
export const useSystemInfo = () => useQuery({ queryKey: ["system"], queryFn: () => api<SystemInfo | null>("/system/info") });
export const useOverview = () => useQuery({ queryKey: ["overview"], queryFn: () => api<Overview | null>("/overview") });
export const useNodes = () => useQuery({ queryKey: ["nodes"], queryFn: () => api<NodeRow[]>("/nodes") });
export const useTimeline = (id: number | null) =>
  useQuery({ queryKey: ["nodes", id, "timeline"], queryFn: () => api<EventDetail[]>(`/nodes/${String(id)}/timeline`), enabled: id !== null });
export const useConfigs = (kind: string) => useQuery({ queryKey: ["configs", kind], queryFn: () => api<ConfigObject[]>(`/configs/${kind}`) });
export function useConfigHead<T>(kind: string, id: number | null) {
  return useQuery({ queryKey: ["configs", kind, id], queryFn: () => api<ConfigHead<T>>(`/configs/${kind}/${String(id)}`), enabled: id !== null });
}
export const usePresets = () => useQuery({ queryKey: ["presets"], queryFn: () => api<Presets>("/presets"), staleTime: Infinity });
export const useInitModules = () => useQuery({ queryKey: ["init-modules"], queryFn: () => api<InitModules>("/init-modules"), staleTime: Infinity });
export const useAssets = () => useQuery({ queryKey: ["assets"], queryFn: () => api<AssetsView | null>("/assets") });
export const useIsos = () => useQuery({ queryKey: ["isos"], queryFn: () => api<IsoList | null>("/isos") });
export const useIsoTree = (sha: string | null) => useQuery({ queryKey: ["isos", sha, "tree"], queryFn: () => api<IsoTree>(`/isos/${sha ?? ""}/tree`), enabled: sha !== null && sha !== "", staleTime: Infinity });
export const useBootMenuStatus = () => useQuery({ queryKey: ["boot-menu", "status"], queryFn: () => api<BootMenuStatus | null>("/boot-menu/status") });
export const useBootnetStatus = () => useQuery({ queryKey: ["bootnet", "status"], queryFn: () => api<BootnetStatus | null>("/bootnet/status") });
export const useObservations = () => useQuery({ queryKey: ["bootnet", "observations"], queryFn: () => api<Observation[]>("/bootnet/observations") });
export const useSnippets = (enabled: boolean) => useQuery({ queryKey: ["bootnet", "snippets"], queryFn: () => api<Snippets | null>("/bootnet/snippets"), enabled });
export const useHistory = () => useQuery({ queryKey: ["history"], queryFn: () => api<HistoryRow[]>("/history") });
export const useAudit = (params: string) => useQuery({ queryKey: ["audit", params], queryFn: () => api<AuditPage | null>(`/audit?${params}`), placeholderData: (previous) => previous });
export const useUsers = (enabled: boolean) => useQuery({ queryKey: ["users"], queryFn: () => api<UserSummary[]>("/users"), enabled });
export const useSessions = () => useQuery({ queryKey: ["me", "sessions"], queryFn: () => api<SessionInfo[]>("/me/sessions") });
export const useTokens = () => useQuery({ queryKey: ["me", "tokens"], queryFn: () => api<TokenInfo[]>("/me/tokens") });

export function errorText(error: unknown): string {
  if (error instanceof ApiError) return `${error.code}：${error.message}`;
  return error instanceof Error ? error.message : String(error);
}

export interface ActionSpec<T> {
  /** 成功提示。 */
  success: string;
  /** 失败提示标题。 */
  failure?: string;
  /** 完成后标记失效的查询。 */
  invalidate?: readonly QueryKey[];
  run: () => Promise<T>;
}

/** 写操作：成功/失败 Toast + 失效相关查询；返回结果（失败时为 undefined）。 */
export function useAction() {
  const client = useQueryClient();
  const mutation = useMutation({ mutationFn: (run: () => Promise<unknown>) => run() });
  const { mutateAsync } = mutation;
  const perform = useCallback(
    async <T,>(spec: ActionSpec<T>): Promise<T | undefined> => {
      try {
        const result = (await mutateAsync(spec.run)) as T;
        notify(spec.success, "success");
        for (const key of spec.invalidate ?? []) void client.invalidateQueries({ queryKey: key });
        return result;
      } catch (error) {
        notify(spec.failure ?? "操作失败", "danger", errorText(error));
        return undefined;
      }
    },
    [client, mutateAsync],
  );
  return { perform, busy: mutation.isPending };
}
