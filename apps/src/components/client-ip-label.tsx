"use client";

import { formatClientIpName } from "@/lib/client-ip-names";
import { useI18n } from "@/lib/i18n/provider";
import { useAppStore } from "@/lib/store/useAppStore";

export function ClientIpLabel({ ip, emptyLabel = "-" }: { ip: string | null | undefined; emptyLabel?: string }) {
  const { t } = useI18n();
  const names = useAppStore(state => state.appSettings.clientIpNames);
  const text = ip?.trim() ? formatClientIpName(ip.trim(), names, t("未绑定")) : emptyLabel;
  return <span className="shrink-0 whitespace-nowrap" title={text}>{text}</span>;
}
