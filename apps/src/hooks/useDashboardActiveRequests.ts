"use client";

import { useQuery } from "@tanstack/react-query";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { dashboardClient } from "@/lib/api/dashboard-client";
import { useAppStore } from "@/lib/store/useAppStore";
import type { DashboardActiveRequests } from "@/types";

export const DASHBOARD_ACTIVE_REQUESTS_QUERY_KEY = [
  "dashboard",
  "active-requests",
] as const;

export function useDashboardActiveRequests(
  params?: { limit?: number | null },
  enabled = true,
) {
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const isPageActive = useDesktopPageActive("/");
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const isServiceReady = serviceStatus.connected;
  const isQueryEnabled = useDeferredDesktopActivation(
    enabled && isServiceReady && isPageActive && isDesktopRuntime,
  );

  const query = useQuery<DashboardActiveRequests>({
    queryKey: [
      ...DASHBOARD_ACTIVE_REQUESTS_QUERY_KEY,
      serviceStatus.addr,
      params?.limit ?? null,
    ],
    queryFn: () =>
      dashboardClient.getActiveRequests({
        limit: params?.limit ?? null,
      }),
    enabled: isQueryEnabled,
    refetchInterval: isQueryEnabled ? 1500 : false,
    retry: 1,
    staleTime: 1000,
  });

  return {
    ...query,
    isServiceReady,
    isDesktopRuntime,
  };
}
