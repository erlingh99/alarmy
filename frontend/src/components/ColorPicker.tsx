
import React, { useState } from "react";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { presetColors } from "@/consts"

interface ColorPickerProps {
  value: string;
  onChange: (color: string) => void;
  className?: string;
}

export const ColorPicker: React.FC<ColorPickerProps> = ({
  value,
  onChange,
  className,
}) => {
  const [hexInput, setHexInput] = useState(value);
  const [prevValue, setPrevValue] = useState(value);

  if (prevValue !== value) {
    setPrevValue(value);
    setHexInput(value);
  }

  const handleColorChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newColor = e.target.value;
    onChange(newColor);
  };

  const handleHexInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const newHex = e.target.value;
    setHexInput(newHex);

    // Validate hex code format (#RRGGBB or #RGB)
    if (/^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$/.test(newHex)) {
      onChange(newHex);
    }
  };

  // Ensure value is a valid hex color
  const safeColor = /^#([A-Fa-f0-9]{6}|[A-Fa-f0-9]{3})$/.test(value) ? value : "#000000";

  return (
    <div className={cn("space-y-2", className)}>
      <Label htmlFor="color">Alarm Color</Label>
      
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="relative flex-shrink-0">
          <Input 
            type="color" 
            id="color" 
            value={safeColor} 
            onChange={handleColorChange}
            className="h-10 w-10 p-1 cursor-pointer bg-transparent border-input overflow-hidden"
          />
        </div>
        
        <div className="flex-grow">
          <Input
            type="text"
            placeholder="#RRGGBB"
            value={hexInput}
            onChange={handleHexInputChange}
            className="font-mono"
            maxLength={7}
          />
        </div>
      </div>
      
      <div className="flex flex-wrap gap-2">
        {presetColors.map((color) => (
          <Button
            key={color}
            type="button"
            onClick={() => onChange(color)}
            className="w-8 h-8 p-0 rounded-full"
            style={{ backgroundColor: color }}
            variant="outline"
            aria-label={`Select color ${color}`}
          />
        ))}
      </div>
      
      <p className="text-xs text-muted-foreground">
        Choose a color from the color wheel, select a preset, or enter a valid hex code (e.g., #FF5733)
      </p>
    </div>
  );
};
