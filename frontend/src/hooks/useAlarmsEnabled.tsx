import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { ModeService } from "@/services/modeService";
import { TOAST_DURATION } from "@/consts";

const modeService = ModeService.getInstance();

export function useAlarmsEnabled() {
  const queryClient = useQueryClient();

  // Defaults to true while loading so the disabled banner never flashes on a
  // normal page load.
  const { data: alarmsEnabled = true, isLoading } = useQuery({
    queryKey: ["alarms-enabled"],
    queryFn: () => modeService.getAlarmsEnabled(),
    meta: {
      onError: (error: Error) => {
        console.error("Error fetching alarm state:", error);
      },
    },
  });

  const mutation = useMutation({
    mutationFn: (enabled: boolean) => modeService.setAlarmsEnabled(enabled),
    onMutate: async (enabled) => {
      await queryClient.cancelQueries({ queryKey: ["alarms-enabled"] });
      const previousEnabled = queryClient.getQueryData<boolean>(["alarms-enabled"]);
      queryClient.setQueryData(["alarms-enabled"], enabled);
      return { previousEnabled };
    },
    onSuccess: (confirmedEnabled) => {
      // Write the server-confirmed value straight into the cache instead of
      // invalidating: a follow-up refetch can dedupe with an already in-flight
      // (pre-mutation) request and silently leave the UI stale.
      queryClient.setQueryData(["alarms-enabled"], confirmedEnabled);
      toast.success(confirmedEnabled ? "Alarms enabled" : "Alarms disabled", {
        duration: TOAST_DURATION,
      });
    },
    onError: (error: Error, _enabled, context) => {
      if (context?.previousEnabled !== undefined) {
        queryClient.setQueryData(["alarms-enabled"], context.previousEnabled);
      }
      toast.error(`Failed to change alarm state: ${error.message}`, {
        duration: TOAST_DURATION,
      });
    },
  });

  return {
    alarmsEnabled,
    isLoading,
    setAlarmsEnabled: mutation.mutate,
    isToggling: mutation.isPending,
  };
}
