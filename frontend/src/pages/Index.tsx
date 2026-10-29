
import { useState } from "react";
import { AlarmList } from "@/components/AlarmList";
import { AlarmHeader } from "@/components/AlarmHeader";
import { Alarm } from "@/types/alarm";
import { useAlarmMutations } from "@/hooks/useAlarmMutations";
import { useAlarms } from "@/hooks/useAlarms";
import { useAlarmsEnabled } from "@/hooks/useAlarmsEnabled";
import { AlarmsDisabledBanner } from "@/components/AlarmsDisabledBanner";

const Index = () => {
  const [editingAlarm, setEditingAlarm] = useState<Alarm | null>(null);
  const [isDialogOpen, setIsDialogOpen] = useState(false);
  const { alarms, isLoading } = useAlarms();
  const { alarmsEnabled } = useAlarmsEnabled();
  const {
    createAlarmMutation,
    updateAlarmMutation,
    deleteAlarmMutation
  } = useAlarmMutations();

  const handleSubmit = async (data: {
    name: string;
    time: string;
    color: string;
    length: number;
    intensity_curve: Alarm['intensity_curve'];
    recurrence: Alarm['recurrence'];
    play_melody: boolean;
  }) => {
    try {
      if (editingAlarm) {
        await updateAlarmMutation.mutateAsync({ id: editingAlarm.id, updates: data, optimistic: false });
      } else {
        await createAlarmMutation.mutateAsync(data);
      }
      setIsDialogOpen(false);
    } catch {
      // error toast is shown by onError; dialog stays open so the user can retry
    }
  };

  const handleEdit = (alarm: Alarm) => {
    setEditingAlarm(alarm);
    setIsDialogOpen(true);
  };

  const handleToggle = (id: string, isActive: boolean) => {
    updateAlarmMutation.mutate({
      id,
      updates: { is_active: isActive },
      silent: true,
    });
  };

  return (
    <div className="container max-w-2xl py-8 space-y-8">
      <AlarmHeader
        isDialogOpen={isDialogOpen}
        setIsDialogOpen={setIsDialogOpen}
        editingAlarm={editingAlarm}
        setEditingAlarm={setEditingAlarm}
        onSubmit={handleSubmit}
        isSubmitting={createAlarmMutation.isPending || updateAlarmMutation.isPending}
      />

      <AlarmsDisabledBanner />

      {isLoading ? (
        <div className="flex justify-center py-12 text-muted-foreground text-sm">
          Loading alarms…
        </div>
      ) : (
        <AlarmList
          alarms={alarms}
          onToggle={handleToggle}
          onEdit={handleEdit}
          onDelete={(id) => deleteAlarmMutation.mutate(id)}
          dimmed={!alarmsEnabled}
        />
      )}
    </div>
  );
};

export default Index;
