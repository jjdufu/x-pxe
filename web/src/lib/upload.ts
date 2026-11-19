// ISO 分块上传（断点续传）：开始 → 按偏移逐块 PUT → 完成。偏移不符（409）时先查询已收字节再续传；
// 同一文件（名称 + 大小 + 修改时间）的未完成上传 ID 经 storage.ts 记在 localStorage，刷新页面后可继续。
import { api, ApiError } from "./api";
import { readItem, removeItem, writeItem } from "./storage";
import type { IsoDetection } from "./types";

export const CHUNK_BYTES = 8 * 1024 * 1024;
const STORE_PREFIX = "xpxe.iso-upload:";

export interface UploadResult {
  id: number;
  sha256: string;
  size: number;
  label: string;
  detection: IsoDetection;
}

interface Progress {
  id: string;
  size: number;
  received: number;
}

function fileKey(file: File): string {
  return `${STORE_PREFIX}${file.name}:${String(file.size)}:${String(file.lastModified)}`;
}

/** 已记录的上传仍存在时返回其进度，否则开始新上传。 */
async function resumeOrBegin(file: File, label: string, expected: string): Promise<Progress> {
  const key = fileKey(file);
  const known = readItem(key);
  if (known !== null) {
    try {
      const progress = await api<Progress>(`/isos/uploads/${known}`);
      if (progress.size === file.size) return progress;
    } catch (error) {
      if (!(error instanceof ApiError) || error.status !== 404) throw error;
    }
  }
  const begun = await api<Progress>("/isos/uploads", { method: "POST", body: { label, size: file.size, expected_sha256: expected === "" ? null : expected } });
  writeItem(key, begun.id);
  return begun;
}

/** 上传一个 ISO；onProgress 以 0–1 报告进度。 */
export async function uploadIso(file: File, label: string, expected: string, onProgress: (ratio: number) => void, signal?: AbortSignal): Promise<UploadResult> {
  const state = await resumeOrBegin(file, label, expected.trim().toLowerCase());
  let offset = state.received;
  onProgress(file.size === 0 ? 1 : offset / file.size);
  while (offset < file.size) {
    if (signal?.aborted === true) throw new ApiError(0, "ABORTED", "已取消");
    const end = Math.min(offset + CHUNK_BYTES, file.size);
    try {
      const reply = await api<Progress>(`/isos/uploads/${state.id}?offset=${String(offset)}`, { method: "PUT", raw: file.slice(offset, end), ...(signal === undefined ? {} : { signal }) });
      offset = reply.received;
    } catch (error) {
      if (!(error instanceof ApiError) || error.code !== "OFFSET_MISMATCH") throw error;
      offset = (await api<Progress>(`/isos/uploads/${state.id}`)).received;
    }
    onProgress(offset / file.size);
  }
  const result = await api<UploadResult>(`/isos/uploads/${state.id}/complete`, { method: "POST", body: {} });
  removeItem(fileKey(file));
  return result;
}
