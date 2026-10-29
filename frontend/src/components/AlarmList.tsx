
import { memo } from "react";
import { Alarm } from "@/types/alarm";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { Pencil, Trash2, Clock } from "lucide-react";
interface AlarmListProps {
  alarms: Alarm[];
  onToggle: (id: string, isActive: boolean) => void;
  onEdit: (alarm: Alarm) => void;
  onDelete: (id: string) => void;
  /** Alarms are globally disabled, so none of these will fire. Visual only
   *  the per-alarm controls stay usable. */
  dimmed?: boolean;
}

const formatRecurrence = (alarm: Alarm) => {
  const { recurrence_type: type, days, custom_dates: customDates } = alarm.recurrence;
  if (type === "daily") return "Every day";
  if (type === "weekly" && days) {
    const weekDayNames = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    return `Every ${weekDayNames.filter((_v, i) => days[i]).join(", ")}`;
  }
  if (type === "custom" && customDates) {
    const sortedDates = [...customDates].sort((a, b) =>
      new Date(a).getTime() - new Date(b).getTime()
    );
    return sortedDates
      .map(date => new Date(date).toLocaleDateString('en-GB', { day: 'numeric', month: 'short' }))
      .join(", ");
  }
  return "Something went wrong";
};

function formatTime(time: string) {
  return time.substring(0, 5);
}

interface AlarmItemProps {
  alarm: Alarm;
  onToggle: (id: string, isActive: boolean) => void;
  onEdit: (alarm: Alarm) => void;
  onDelete: (id: string) => void;
}

const AlarmItem = memo(({ alarm, onToggle, onEdit, onDelete }: AlarmItemProps) => (
  <div
    className="flex items-center justify-between p-4 rounded-lg bg-card/50 backdrop-blur-sm border border-border/50 shadow-sm hover:shadow-md transition-all"
    style={{ borderLeft: `4px solid ${alarm.color}` }}
  >
    <div className="flex-1">
      <div className="flex items-center">
        <div
          className="w-3 h-3 rounded-full mr-2"
          style={{ backgroundColor: alarm.color }}
        />
        <h3 className="font-medium">{alarm.name}</h3>
      </div>
      <div className="flex flex-wrap gap-4 text-sm text-muted-foreground">
        <span>{formatTime(alarm.time)}</span>
        -
        <span>{formatRecurrence(alarm)}</span>
      </div>
      <div className="text-xs text-muted-foreground mt-1">
        <span className="flex items-center">
          <Clock className="h-3 w-3 mr-1" />
          {alarm.length} min
        </span>
      </div>
      <div className="text-xs text-muted-foreground mt-1">
        <span>Intensity: {alarm.intensity_curve.start_intensity}% → {alarm.intensity_curve.end_intensity}% ({alarm.intensity_curve.curve})</span>
      </div>
    </div>

    <div className="flex items-center gap-4">
      <Switch
        checked={alarm.is_active}
        onCheckedChange={(checked) => onToggle(alarm.id, checked)}
      />
      <Button variant="ghost" size="icon" onClick={() => onEdit(alarm)}>
        <Pencil className="h-4 w-4" />
      </Button>
      <Button variant="ghost" size="icon" onClick={() => onDelete(alarm.id)}>
        <Trash2 className="h-4 w-4 text-destructive" />
      </Button>
    </div>
  </div>
));

export const AlarmList = ({ alarms, onToggle, onEdit, onDelete, dimmed = false }: AlarmListProps) => {
  if (alarms.length === 0) {
    return (
      <div className="text-center py-8 text-muted-foreground animate-fade-in">
        No alarms set yet
      </div>
    );
  }

  return (
    <div
      className={
        dimmed
          ? "space-y-4 animate-fade-in opacity-50 transition-opacity"
          : "space-y-4 animate-fade-in transition-opacity"
      }
    >
      {alarms.map((alarm) => (
        <AlarmItem
          key={alarm.id}
          alarm={alarm}
          onToggle={onToggle}
          onEdit={onEdit}
          onDelete={onDelete}
        />
      ))}
    </div>
  );
};
