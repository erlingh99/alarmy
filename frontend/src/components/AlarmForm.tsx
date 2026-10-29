
import { useState, type FormEvent } from "react";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { TimePicker } from "./TimePicker";
import { RecurrenceSelector } from "./RecurrenceSelector";
import { RecurrencePattern, IntensityCurve } from "@/types/alarm";
import { toast } from "sonner";
import { ColorPicker } from "./ColorPicker";
import { AlarmLengthSelector } from "./AlarmLengthSelector";
import { IntensityCurveSelector } from "./IntensityCurveSelector";
import * as consts from "@/consts"

interface AlarmFormProps {
  onSubmit: (data: {
    name: string;
    time: string;
    color: string;
    length: number;
    intensity_curve: IntensityCurve;
    recurrence: RecurrencePattern;
    play_melody: boolean;
  }) => void;
  initialData?: {
    name: string;
    time: string;
    color: string;
    length?: number;
    intensity_curve?: IntensityCurve;
    recurrence: RecurrencePattern;
    play_melody?: boolean;
  };
  isPending?: boolean;
}

const TIME_RE = /^\d{2}:\d{2}$/;
const COLOR_RE = /^#[0-9A-Fa-f]{6}$/;

export function AlarmForm({ onSubmit, initialData, isPending }: AlarmFormProps) {
  const [name, setName] = useState(initialData?.name || consts.INITIAL_ALARM_NAME);
  const [time, setTime] = useState(initialData?.time || consts.INITIAL_ALARM_TIME);
  const [color, setColor] = useState(initialData?.color || consts.INITIAL_ALARM_COLOR);
  const [length, setLength] = useState(initialData?.length || consts.INITIAL_ALARM_LENGTH);
  const [intensityCurve, setIntensityCurve] = useState<IntensityCurve>(
    initialData?.intensity_curve || {
      start_intensity: consts.INITIAL_START_INTENSITY,
      end_intensity: consts.INITIAL_END_INTENSITY,
      curve: consts.INITIAL_INTENSITY_TYPE,
    }
  );
  const [recurrence, setRecurrence] = useState<RecurrencePattern>(
    initialData?.recurrence || { recurrence_type: "daily" }
  );
  const [playMelody, setPlayMelody] = useState(
    initialData?.play_melody ?? consts.INITIAL_PLAY_MELODY
  );

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();

    if (!name.trim()) {
      toast.error("Please enter an alarm name", { duration: consts.TOAST_DURATION });
      return;
    }

    if (!TIME_RE.test(time.substring(0, 5))) {
      toast.error("Invalid time format", { duration: consts.TOAST_DURATION });
      return;
    }

    if (!COLOR_RE.test(color)) {
      toast.error("Invalid color format — expected #RRGGBB", { duration: consts.TOAST_DURATION });
      return;
    }

    if (length < 1 || length > 255) {
      toast.error("Alarm length must be between 1 and 255 minutes", { duration: consts.TOAST_DURATION });
      return;
    }

    if (intensityCurve.start_intensity < 0 || intensityCurve.start_intensity > 100 ||
        intensityCurve.end_intensity < 0 || intensityCurve.end_intensity > 100) {
      toast.error("Intensity values must be between 0 and 100", { duration: consts.TOAST_DURATION });
      return;
    }

    if (recurrence.recurrence_type === "weekly" && (!recurrence.days || !recurrence.days.some(x => x))) {
      toast.error("Please select at least one day for weekly alarm", { duration: consts.TOAST_DURATION });
      return;
    }

    if (recurrence.recurrence_type === "custom" && (!recurrence.custom_dates || recurrence.custom_dates.length === 0)) {
      toast.error("Please select at least one date for custom alarm", { duration: consts.TOAST_DURATION });
      return;
    }

    onSubmit({ name, time, color, length, intensity_curve: intensityCurve, recurrence, play_melody: playMelody });
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-6 animate-fade-in">
      <div>
        <Label htmlFor="name">Alarm Name</Label>
        <Input
          id="name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={consts.ALARM_TOOLTIP}
          className="mt-1"
        />
      </div>

      <TimePicker value={time} onChange={setTime} />

      <ColorPicker value={color} onChange={setColor} />

      <AlarmLengthSelector value={length} onChange={setLength} />

      <IntensityCurveSelector
        value={intensityCurve}
        onChange={setIntensityCurve}
      />

      <RecurrenceSelector value={recurrence} onChange={setRecurrence} />

      <div className="flex items-center justify-between">
        <Label htmlFor="play-melody">Play melody at the end of the alarm</Label>
        <Switch
          id="play-melody"
          checked={playMelody}
          onCheckedChange={setPlayMelody}
        />
      </div>

      <Button type="submit" className="w-full" disabled={isPending}>
        {isPending && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
        {isPending ? "Saving…" : "Save Alarm"}
      </Button>
    </form>
  );
}
