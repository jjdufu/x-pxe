// 登录页（step-12 §3.1）：不使用外壳；失败统一错误；退避期间按钮禁用并倒数；登录后若须改密由外壳弹出不可关闭的改密对话框。
import { useQueryClient } from "@tanstack/react-query";
import { Eye, EyeOff, LogIn } from "lucide-react";
import { useEffect, useState, type SyntheticEvent } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { Button } from "../../components/ui/button";
import { Alert } from "../../components/ui/feedback";
import { Field, Input } from "../../components/ui/field";
import { api, ApiError } from "../../lib/api";
import type { Me } from "../../lib/types";

export function LoginPage() {
  const [username, setUsername] = useState("admin");
  const [password, setPassword] = useState("");
  const [reveal, setReveal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [wait, setWait] = useState(0);
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const client = useQueryClient();

  useEffect(() => {
    if (wait <= 0) return undefined;
    const timer = window.setTimeout(() => {
      setWait(wait - 1);
    }, 1000);
    return () => {
      window.clearTimeout(timer);
    };
  }, [wait]);

  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const reply = await api<Me>("/auth/login", { method: "POST", body: { username, password } });
      client.setQueryData(["me"], { user: reply.user, role: reply.user.role });
      await client.invalidateQueries();
      const next = params.get("next");
      void navigate(next !== null && next.startsWith("/") && !next.startsWith("//") && !next.startsWith("/login") ? next : "/");
    } catch (cause) {
      if (cause instanceof ApiError && cause.status === 429) {
        setWait(cause.retryAfter ?? 30);
        setError("尝试次数过多，请稍后再试");
      } else {
        setError("用户名或密码错误");
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="login-wrap">
      <form
        className="ui-card login-card"
        onSubmit={(event) => {
          void submit(event);
        }}
      >
        <div className="brand">
          <span className="brand-mark" aria-hidden="true">
            x
          </span>
          <div>
            <h1 className="brand-title">x-pxe</h1>
            <div className="brand-sub">Debian 13 自动装机</div>
          </div>
        </div>
        {error !== null && (
          <Alert tone="danger" title="登录失败">
            {error}
          </Alert>
        )}
        <Field label="用户名" required>
          <Input
            autoComplete="username"
            value={username}
            onChange={(event) => {
              setUsername(event.target.value);
            }}
          />
        </Field>
        <Field label="密码" required>
          <div className="input-group">
            <Input
              type={reveal ? "text" : "password"}
              autoComplete="current-password"
              value={password}
              onChange={(event) => {
                setPassword(event.target.value);
              }}
            />
            <button
              type="button"
              className="input-action-btn"
              aria-label={reveal ? "隐藏密码" : "显示密码"}
              onClick={() => {
                setReveal(!reveal);
              }}
            >
              {reveal ? <EyeOff size={14} aria-hidden="true" /> : <Eye size={14} aria-hidden="true" />}
            </button>
          </div>
        </Field>
        <Button type="submit" variant="primary" disabled={busy || wait > 0 || password === ""}>
          {busy ? <span className="spinner" aria-hidden="true" /> : <LogIn size={14} aria-hidden="true" />}
          {wait > 0 ? <span className="tabular">请等待 {wait} 秒</span> : "登录"}
        </Button>
      </form>
    </div>
  );
}
