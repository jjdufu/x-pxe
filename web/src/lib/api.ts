// 管理 API 客户端：同源、带会话 Cookie；写操作从 xpxe_csrf Cookie 读回 CSRF 令牌放进 X-CSRF-Token。
// 401 时广播未登录信号（登录接口自身除外），由外壳跳转登录页。
import { signalUnauthorized } from "./auth-bus";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly fields: unknown;
  /** 429 时的 Retry-After 秒数。 */
  readonly retryAfter: number | null;

  constructor(status: number, code: string, message: string, fields: unknown = null, retryAfter: number | null = null) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.fields = fields;
    this.retryAfter = retryAfter;
  }
}

const SAFE = new Set(["GET", "HEAD", "OPTIONS"]);

/** 读取 CSRF Cookie。 */
export function csrfToken(cookies: string = document.cookie): string | null {
  for (const part of cookies.split(";")) {
    const [name, ...rest] = part.trim().split("=");
    if (name === "xpxe_csrf") {
      const value = rest.join("=");
      return value === "" ? null : decodeURIComponent(value);
    }
  }
  return null;
}

interface ErrorBody {
  error?: { code?: string; message?: string; fields?: unknown };
}

function isErrorBody(value: unknown): value is ErrorBody {
  return typeof value === "object" && value !== null && "error" in value;
}

function retryAfterOf(response: Response): number | null {
  const raw = response.headers.get("Retry-After");
  if (raw === null) return null;
  const value = Number.parseInt(raw, 10);
  return Number.isNaN(value) ? null : value;
}

export interface RequestOptions {
  method?: string;
  body?: unknown;
  /** 原始请求体（上传文件），以 application/octet-stream 发送。 */
  raw?: Blob;
  signal?: AbortSignal;
}

/** 发起请求；非 2xx 抛 ApiError（错误码取响应体 error.code，缺省为 HTTP_<status>）。 */
export async function api<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const method = options.method ?? "GET";
  const headers = new Headers({ Accept: "application/json" });
  if (!SAFE.has(method)) {
    const csrf = csrfToken();
    if (csrf !== null) headers.set("X-CSRF-Token", csrf);
  }
  const init: RequestInit = { method, headers, credentials: "same-origin" };
  if (options.raw !== undefined) {
    headers.set("Content-Type", "application/octet-stream");
    init.body = options.raw;
  } else if (options.body !== undefined) {
    headers.set("Content-Type", "application/json");
    init.body = JSON.stringify(options.body);
  }
  if (options.signal !== undefined) init.signal = options.signal;
  let response: Response;
  try {
    response = await fetch(`/api/v1${path}`, init);
  } catch (cause) {
    throw new ApiError(0, "NETWORK", cause instanceof Error ? cause.message : "网络错误");
  }
  if (response.status === 204) return undefined as T;
  const text = await response.text();
  let parsed: unknown = null;
  if (text !== "") {
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
  }
  if (!response.ok) {
    if (response.status === 401 && path !== "/auth/login") signalUnauthorized();
    const detail = isErrorBody(parsed) ? parsed.error : undefined;
    throw new ApiError(
      response.status,
      detail?.code ?? `HTTP_${String(response.status)}`,
      detail?.message ?? response.statusText,
      detail?.fields ?? null,
      retryAfterOf(response),
    );
  }
  return parsed as T;
}
