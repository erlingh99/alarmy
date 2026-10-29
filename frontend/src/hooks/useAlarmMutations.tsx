import { useMutation, useQueryClient } from "@tanstack/react-query";
import { alarmService } from "@/services/alarmService";
import { Alarm, IntensityCurve, RecurrencePattern } from "@/types/alarm";
import { toast } from "sonner";
import { TOAST_DURATION, RESTORE_TOAST_DURATION } from "@/consts";

// Module-level store for recently deleted alarms (enables undo without a network fetch)
const deletedAlarmsStore = new Map<string, Alarm>();

export function useAlarmMutations() {
  const queryClient = useQueryClient();

  const createAlarmMutation = useMutation({
    mutationFn: (data: {
      name: string;
      time: string;
      color: string;
      length: number;
      intensity_curve: IntensityCurve;
      recurrence: RecurrencePattern;
      play_melody: boolean;
    }) =>
      alarmService.createAlarm(
        data.name,
        data.time,
        data.recurrence,
        data.color,
        data.length,
        data.intensity_curve,
        data.play_melody
      ),
    onSuccess: (newAlarm) => {
      queryClient.setQueryData<Alarm[]>(["alarms"], (old = []) => [...old, newAlarm]);
      toast.success("Alarm created successfully", { duration: TOAST_DURATION });
    },
    onError: (error: Error) => {
      console.error("Error creating alarm:", error);
      toast.error(`Failed to create alarm: ${error.message}`, { duration: TOAST_DURATION });
    },
  });

  const updateAlarmMutation = useMutation({
    mutationFn: (data: { id: string; updates: Partial<Alarm>; silent?: boolean; optimistic?: boolean }) =>
      alarmService.updateAlarm(data.id, data.updates),
    onMutate: async ({ id, updates, optimistic = true }) => {
      if (!optimistic) return;

      await queryClient.cancelQueries({ queryKey: ["alarms"] });
      const previousAlarms = queryClient.getQueryData<Alarm[]>(["alarms"]);

      queryClient.setQueryData<Alarm[]>(["alarms"], (old = []) =>
        old.map((a) => (a.id === id ? { ...a, ...updates } : a))
      );
      return { previousAlarms };
    },
    onError: (error: Error, _data, context) => {
      if (context?.previousAlarms) {
        queryClient.setQueryData(["alarms"], context.previousAlarms);
      }
      console.error("Error updating alarm:", error);
      toast.error(`Failed to update alarm: ${error.message}`, { duration: TOAST_DURATION });
    },
    onSuccess: (updatedAlarm, { silent }) => {
      queryClient.setQueryData<Alarm[]>(["alarms"], (old = []) =>
        old.map((a) => (a.id === updatedAlarm.id ? updatedAlarm : a))
      );
      if (!silent) {
        toast.success("Alarm updated successfully", { duration: TOAST_DURATION });
      }
    },
  });

  const deleteAlarmMutation = useMutation({
    mutationFn: alarmService.deleteAlarm,
    onMutate: async (id) => {
      await queryClient.cancelQueries({ queryKey: ["alarms"] });
      const previousAlarms = queryClient.getQueryData<Alarm[]>(["alarms"]);

      const deletedAlarm = previousAlarms?.find((a) => a.id === id);
      if (deletedAlarm) {
        deletedAlarmsStore.set(id, deletedAlarm);
      }

      queryClient.setQueryData<Alarm[]>(["alarms"], (old = []) =>
        old.filter((a) => a.id !== id)
      );
      return { previousAlarms };
    },
    onError: (error: Error, id, context) => {
      queryClient.setQueryData(["alarms"], context?.previousAlarms);
      deletedAlarmsStore.delete(id);
      console.error("Error deleting alarm:", error);
      toast.error(`Failed to delete alarm: ${error.message}`, { duration: TOAST_DURATION });
    },
    onSuccess: (_void, deletedAlarmId) => {
      toast.success("Alarm deleted", {
        duration: RESTORE_TOAST_DURATION,
        action: {
          label: "Undo",
          onClick: () => {
            const alarm = deletedAlarmsStore.get(deletedAlarmId);
            if (alarm) restoreAlarmMutation.mutate(alarm);
          },
        },
      });
    },
  });

  const restoreAlarmMutation = useMutation({
    mutationFn: alarmService.restoreAlarm,
    onMutate: async (alarm) => {
      await queryClient.cancelQueries({ queryKey: ["alarms"] });
      const previousAlarms = queryClient.getQueryData<Alarm[]>(["alarms"]);

      const tempId = `temp-restore-${Date.now()}`;
      queryClient.setQueryData<Alarm[]>(["alarms"], (old = []) => [
        ...old,
        { ...alarm, id: tempId },
      ]);
      deletedAlarmsStore.delete(alarm.id);

      return { previousAlarms, tempId };
    },
    onError: (error: Error, alarm, context) => {
      queryClient.setQueryData(["alarms"], context?.previousAlarms);
      deletedAlarmsStore.set(alarm.id, alarm);
      console.error("Error restoring alarm:", error);
      toast.error(`Failed to restore alarm: ${error.message}`, { duration: TOAST_DURATION });
    },
    onSuccess: (restoredAlarm, _alarm, context) => {
      queryClient.setQueryData<Alarm[]>(["alarms"], (old = []) =>
        old.map((a) => (a.id === context?.tempId ? restoredAlarm : a))
      );
      toast.success("Alarm restored successfully", { duration: TOAST_DURATION });
    },
  });

  return {
    createAlarmMutation,
    updateAlarmMutation,
    deleteAlarmMutation,
    restoreAlarmMutation,
  };
}
