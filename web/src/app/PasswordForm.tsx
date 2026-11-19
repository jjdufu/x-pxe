// 修改密码表单：设置页与强制改密对话框共用。
import { useQueryClient } from "@tanstack/react-query";
import { useState, type SyntheticEvent } from "react";
import { Button } from "../components/ui/button";
import { Field, Input } from "../components/ui/field";
import { Dialog } from "../components/ui/dialog";
import { notify } from "../components/ui/toast";
import { api } from "../lib/api";
import { errorText } from "./data";

export function PasswordForm({ onDone }: { onDone?: () => void }) {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [again, setAgain] = useState("");
  const [busy, setBusy] = useState(false);
  const client = useQueryClient();
  const mismatch = again !== "" && again !== next;

  const submit = async (event: SyntheticEvent) => {
    event.preventDefault();
    setBusy(true);
    try {
      await api("/me/password", { method: "POST", body: { old_password: current, new_password: next } });
      notify("密码已修改", "success");
      setCurrent("");
      setNext("");
      setAgain("");
      await client.invalidateQueries({ queryKey: ["me"] });
      onDone?.();
    } catch (error) {
      notify("修改密码失败", "danger", errorText(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      className="page-stack"
      onSubmit={(event) => {
        void submit(event);
      }}
    >
      <Field label="当前密码" required>
        <Input
          type="password"
          autoComplete="current-password"
          value={current}
          onChange={(event) => {
            setCurrent(event.target.value);
          }}
        />
      </Field>
      <Field label="新密码" hint="至少 12 个字符" required>
        <Input
          type="password"
          autoComplete="new-password"
          value={next}
          onChange={(event) => {
            setNext(event.target.value);
          }}
        />
      </Field>
      <Field label="确认新密码" error={mismatch ? "两次输入不一致" : undefined} required>
        <Input
          type="password"
          autoComplete="new-password"
          value={again}
          onChange={(event) => {
            setAgain(event.target.value);
          }}
        />
      </Field>
      <div>
        <Button type="submit" variant="primary" disabled={busy || current === "" || next === "" || again !== next}>
          修改密码
        </Button>
      </div>
    </form>
  );
}

/** 首次登录强制改密：不可关闭。 */
export function ForcedPasswordDialog({ open }: { open: boolean }) {
  return (
    <Dialog open={open} onOpenChange={() => undefined} closable={false} title="请修改初始密码" description="首次登录必须先修改密码才能继续使用。">
      <PasswordForm />
    </Dialog>
  );
}
