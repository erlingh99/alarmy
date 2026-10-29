
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Checkbox } from "@/components/ui/checkbox";
import { Calendar } from "@/components/ui/calendar";
import { RecurrencePattern } from "@/types/alarm";

interface RecurrenceSelectorProps {
  value: RecurrencePattern;
  onChange: (pattern: RecurrencePattern) => void;
}

export function RecurrenceSelector({ value, onChange }: RecurrenceSelectorProps) {
  const days = value.days ?? [false, false, false, false, false, false, false];

  const weekDays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

  const selectedDates = value.custom_dates?.map(date => new Date(date)) || [];

  const handleDateSelect = (dates: Date[] | undefined) => {
    if (!dates) return;
    const sortedDates = [...dates].sort((a, b) => a.getTime() - b.getTime());
    const newDates = sortedDates.map(d => d.toISOString().split('T')[0]);
    onChange({ ...value, custom_dates: newDates });
  };

  const handleDaySelect = (checked: boolean | string, dayIndex: number) => {
    const isChecked = checked === true;
    const newDays = [...days] as [boolean, boolean, boolean, boolean, boolean, boolean, boolean];
    newDays[dayIndex] = isChecked;
    onChange({ ...value, days: newDays });
  };

  return (
    <div className="space-y-4">
      <Label>Repeat</Label>
      <RadioGroup
        value={value.recurrence_type}
        onValueChange={(type: "daily" | "weekly" | "custom") =>
          onChange({ ...value, recurrence_type: type })
        }
        className="flex flex-col gap-2"
      >
        <div className="flex items-center space-x-2">
          <RadioGroupItem value="daily" id="daily" />
          <Label htmlFor="daily">Daily</Label>
        </div>
        <div className="flex items-center space-x-2">
          <RadioGroupItem value="weekly" id="weekly" />
          <Label htmlFor="weekly">Weekly</Label>
        </div>
        <div className="flex items-center space-x-2">
          <RadioGroupItem value="custom" id="custom" />
          <Label htmlFor="custom">Custom</Label>
        </div>
      </RadioGroup>

      {value.recurrence_type === "weekly" && (
        <div className="mt-4">
          <Label className="mb-2 block">Select days</Label>
          <div className="flex gap-2 flex-wrap">
            {weekDays.map((day, index) => (
              <div
                key={day}
                className="flex items-center space-x-2 bg-background/50 p-2 rounded-lg"
              >
                <Checkbox
                  id={day}
                  checked={days[index]}
                  onCheckedChange={(checked) => handleDaySelect(checked, index)}
                />
                <Label htmlFor={day}>{day}</Label>
              </div>
            ))}
          </div>
        </div>
      )}

      {value.recurrence_type === "custom" && (
        <div className="mt-4">
          <Label className="mb-2 block">Select dates</Label>
          <div className="w-full">
            <Calendar
              mode="multiple"
              selected={selectedDates}
              onSelect={handleDateSelect}
              disabled={{ before: new Date() }}
              className="rounded-md border w-full"
            />
          </div>
        </div>
      )}
    </div>
  );
}
