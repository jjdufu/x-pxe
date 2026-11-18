// 管理 API 客户端：同源、带会话 Cookie；写操作从 xpxe_csrf Cookie 读回 CSRF 令牌放进 X-CSRF-Token。
export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly fields: unknown;

  constructor(status: number, code: string, message: string, fields: unknown = null) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.fields = fields;
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

export interface RequestOptions {
  method?: string;
  body?: unknown;
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
  if (options.body !== undefined) {
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
    const detail = isErrorBody(parsed) ? parsed.error : undefined;
    throw new ApiError(
      response.status,
      detail?.code ?? `HTTP_${String(response.status)}`,
      detail?.message ?? response.statusText,
      detail?.fields ?? null,
    );
  }
  return parsed as T;
}
