"use client";

import { useRef, useState } from "react";
import { Check, FileUp, Trash2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/modals/confirm-dialog";
import { useI18n } from "@/lib/i18n/provider";
import { ClientIpNameImportError, readClientIpNameFile, type ClientIpNameImport, type ClientIpNames } from "@/lib/client-ip-names";

export function ClientIpNamesSettings({ names, onSave, saving }: {
  names: ClientIpNames;
  onSave: (names: ClientIpNames) => Promise<unknown>;
  saving: boolean;
}) {
  const { t } = useI18n();
  const inputRef = useRef<HTMLInputElement>(null);
  const [reading, setReading] = useState(false);
  const [preview, setPreview] = useState<(ClientIpNameImport & { filename: string }) | null>(null);
  const [error, setError] = useState("");
  const [confirm, setConfirm] = useState<"replace" | "clear" | null>(null);
  const [filter, setFilter] = useState("");
  const busy = reading || saving || confirm !== null;
  const rows = Object.entries(preview?.names ?? names).filter(([ip, name]) => `${ip} ${name}`.toLowerCase().includes(filter.trim().toLowerCase()));

  async function selectFile(file: File) {
    setReading(true);
    setError("");
    setPreview(null);
    setFilter("");
    try {
      setPreview({ ...await readClientIpNameFile(file), filename: file.name });
    } catch (cause) {
      setError(cause instanceof ClientIpNameImportError
        ? t("第 {row} 行：{message}", { row: cause.row, message: t(cause.message) })
        : cause instanceof Error ? t(cause.message) : t("导入失败"));
    } finally {
      setReading(false);
    }
  }

  async function save() {
    try {
      await onSave(confirm === "clear" ? {} : preview!.names);
      setPreview(null);
      setFilter("");
      setError("");
      return true;
    } catch {
      setError(t("保存失败，原有绑定未更改"));
      return false;
    }
  }

  return (
    <section aria-label={t("IP 人员绑定")} className="space-y-4 border-y border-border/60 py-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold">{t("IP 人员绑定")}</h2>
          <div className="mt-1 text-xs text-muted-foreground">{t("已绑定")} {Object.keys(names).length}</div>
        </div>
        <div className="flex flex-wrap gap-2">
          <input ref={inputRef} type="file" accept=".xlsx" aria-label={t("导入人员表")} className="hidden" disabled={busy}
            onChange={event => { const file = event.target.files?.[0]; event.target.value = ""; if (file) void selectFile(file); }} />
          <Button variant="outline" disabled={busy} onClick={() => inputRef.current?.click()}>
            <FileUp className="h-4 w-4" />{reading ? t("读取中") : t("导入人员表")}
          </Button>
          <Button variant="outline" disabled={busy || !Object.keys(names).length} onClick={() => setConfirm("clear")}>
            <Trash2 className="h-4 w-4" />{t("清空绑定")}
          </Button>
        </div>
      </div>
      {error ? <p role="alert" className="text-sm text-destructive">{error}</p> : null}
      {preview ? <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-0 text-sm">
          <div className="break-all font-medium">{preview.filename}</div>
          <div className="text-xs text-muted-foreground">{t("待导入")} {Object.keys(preview.names).length} · {t("跳过")} {preview.skipped}</div>
        </div>
        <div className="flex gap-2">
          <Button disabled={busy} onClick={() => setConfirm("replace")}><Check className="h-4 w-4" />{t("确认替换")}</Button>
          <Button variant="outline" disabled={busy} onClick={() => { setPreview(null); setError(""); }}><X className="h-4 w-4" />{t("取消")}</Button>
        </div>
      </div> : null}
      <input aria-label={t("搜索 IP 或姓名")} placeholder={t("搜索 IP 或姓名")} value={filter} onChange={event => setFilter(event.target.value)}
        className="h-9 w-full max-w-sm rounded-md border border-input bg-transparent px-3 text-sm" />
      <div className="max-h-72 overflow-auto">
        <table className="w-full text-left text-sm">
          <thead><tr className="border-b border-border/60"><th className="px-2 py-2 font-medium">IP</th><th className="px-2 py-2 font-medium">{t("姓名")}</th></tr></thead>
          <tbody>{rows.slice(0, 100).map(([ip, name]) => <tr key={ip} className="border-b border-border/30">
            <td className="whitespace-nowrap px-2 py-1.5 font-mono">{ip}</td><td className="whitespace-nowrap px-2 py-1.5">{name}</td>
          </tr>)}</tbody>
        </table>
      </div>
      {rows.length > 100 ? <div className="text-xs text-muted-foreground">{t("当前显示")} 100 / {rows.length}</div> : null}
      <ConfirmDialog open={confirm !== null} onOpenChange={open => { if (!open) setConfirm(null); }}
        title={confirm === "clear" ? t("清空绑定") : t("确认替换")}
        description={confirm === "clear" ? t("清空后所有 IP 显示为未绑定，请求不受影响。") : t("将替换现有人员绑定，历史请求日志也按新绑定显示。")}
        confirmVariant={confirm === "clear" ? "destructive" : "default"} onConfirm={save} />
    </section>
  );
}
