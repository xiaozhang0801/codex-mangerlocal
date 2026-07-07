"use client";

import { useQuery } from "@tanstack/react-query";
import { useDeferredDesktopActivation } from "@/hooks/useDeferredDesktopActivation";
import { useDesktopPageActive } from "@/hooks/useDesktopPageActive";
import { useRuntimeCapabilities } from "@/hooks/useRuntimeCapabilities";
import { dashboardClient } from "@/lib/api/dashboard-client";
import { useAppStore } from "@/lib/store/useAppStore";
import type { DashboardActiveRequestsResult } from "@/types";

export const DASHBOARD_ACTIVE_REQUESTS_QUERY_KEY = [
  "dashboard",
  "active-requests",
] as const;

interface DashboardActiveRequestsQueryParams {
  limit?: number;
  enabled?: boolean;
  isAdmin?: boolean;
}

export function useDashboardActiveRequests({
  limit = 20,
  enabled = true,
  isAdmin = false,
}: DashboardActiveRequestsQueryParams = {}) {
  const serviceStatus = useAppStore((state) => state.serviceStatus);
  const { isDesktopRuntime } = useRuntimeCapabilities();
  const isPageActive = useDesktopPageActive("/");
  const isServiceReady = serviceStatus.connected;
  const isQueryEnabled = useDeferredDesktopActivation(
    enabled && isAdmin && isDesktopRuntime && isServiceReady && isPageActive,
  );

  const query = useQuery<DashboardActiveRequestsResult>({
    queryKey: [
      ...DASHBOARD_ACTIVE_REQUESTS_QUERY_KEY,
      serviceStatus.addr,
      limit,
    ],
    queryFn: () => dashboardClient.getActiveRequests({ limit }),
    enabled: isQueryEnabled,
    retry: 1,
    staleTime: 1000,
    refetchInterval: isQueryEnabled ? 1500 : false,
  });

  return {
    ...query,
    isServiceReady,
  };
}
