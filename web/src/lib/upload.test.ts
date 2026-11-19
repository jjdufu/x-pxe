import { afterEach, describe, expect, it, vi } from "vitest";
import { readItem } from "./storage";
import { CHUNK_BYTES, uploadIso } from "./upload";

type Call = { url: string; method: string; size: number };

function server(failFirstPut: boolean) {
  const calls: Call[] = [];
  let received = 0;
  let failed = false;
  const json = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status }));
  const fetchMock = vi.fn((url: string, init: RequestInit) => {
    const method = init.method ?? "GET";
    const size = init.body instanceof Blob ? init.body.size : 0;
    calls.push({ url, method, size });
    if (method === "POST" && url.endsWith("/isos/uploads")) return json({ id: "u1", size: CHUNK_BYTES + 10, received: 0 });
    if (method === "PUT") {
      if (failFirstPut && !failed) {
        failed = true;
        received = CHUNK_BYTES;
        return json({ error: { code: "OFFSET_MISMATCH", message: "已收" } }, 409);
      }
      received += size;
      return json({ id: "u1", size: CHUNK_BYTES + 10, received });
    }
    if (method === "GET") return json({ id: "u1", size: CHUNK_BYTES + 10, received });
    return json({ id: 1, sha256: "a".repeat(64), size: CHUNK_BYTES + 10, label: "x", detection: { family: "pve" } });
  });
  return { calls, fetchMock };
}

describe("uploadIso", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("按 8 MiB 分块上传并完成", async () => {
    const { calls, fetchMock } = server(false);
    vi.stubGlobal("fetch", fetchMock);
    const ratios: number[] = [];
    const file = new File([new Uint8Array(CHUNK_BYTES + 10)], "pve.iso");
    const result = await uploadIso(file, "PVE", "", (ratio) => ratios.push(ratio));
    expect(result.detection.family).toBe("pve");
    expect(calls.filter((call) => call.method === "PUT").map((call) => call.size)).toEqual([CHUNK_BYTES, 10]);
    expect(calls.at(-1)?.url).toBe("/api/v1/isos/uploads/u1/complete");
    expect(ratios.at(-1)).toBe(1);
    expect(readItem(`xpxe.iso-upload:pve.iso:${String(file.size)}:${String(file.lastModified)}`)).toBeNull();
  });

  it("偏移不符时查询已收字节后续传", async () => {
    const { calls, fetchMock } = server(true);
    vi.stubGlobal("fetch", fetchMock);
    const file = new File([new Uint8Array(CHUNK_BYTES + 10)], "pve.iso");
    await uploadIso(file, "PVE", "", () => undefined);
    const puts = calls.filter((call) => call.method === "PUT").map((call) => call.url);
    expect(puts).toEqual([`/api/v1/isos/uploads/u1?offset=0`, `/api/v1/isos/uploads/u1?offset=${String(CHUNK_BYTES)}`]);
  });
});
