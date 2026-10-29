
import { type ChangeEvent } from "react";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Input } from "@/components/ui/input";

interface AlarmLengthSelectorProps {
  value: number;
  onChange: (length: number) => void;
}

export function AlarmLengthSelector({ value, onChange }: AlarmLengthSelectorProps) {
  const handleSliderChange = (values: number[]) => {
    onChange(values[0]);
  };

  const handleInputChange = (e: ChangeEvent<HTMLInputElement>) => {
    const val = parseInt(e.target.value);
    if (!isNaN(val) && val > 0 && val <= 60) {
      onChange(val);
    }
  };

  return (
    <div className="space-y-4">
      <div>
        <Label htmlFor="alarmLength">Alarm Length (minutes)</Label>
        <div className="flex items-center gap-4 mt-2">
          <Slider
            id="alarmLengthSlider"
            value={[value]}
            min={1}
            max={60}
            step={1}
            onValueChange={handleSliderChange}
            className="flex-grow"
          />
          <Input
            id="alarmLength"
            type="number"
            min={1}
            max={60}
            value={value}
            onChange={handleInputChange}
            className="w-20"
          />
        </div>
      </div>
    </div>
  );
}
