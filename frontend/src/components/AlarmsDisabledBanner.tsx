import { BellOff } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useAlarmsEnabled } from "@/hooks/useAlarmsEnabled";

export function AlarmsDisabledBanner() {
  const { alarmsEnabled, setAlarmsEnabled, isToggling } = useAlarmsEnabled();

  if (alarmsEnabled) return null;

  return (
    <div
      role="alert"
      className="flex items-center justify-between gap-4 rounded-lg border border-destructive/50 bg-destructive/10 p-4"
    >
      <div className="flex items-center gap-3">
        <BellOff className="h-5 w-5 shrink-0 text-destructive" />
        <div>
          <p className="font-medium">Alarms are disabled</p>
          <p className="text-sm text-muted-foreground">
            None of your alarms will go off until you turn them back on.
          </p>
        </div>
      </div>
      <Button
        variant="outline"
        size="sm"
        disabled={isToggling}
        onClick={() => setAlarmsEnabled(true)}
      >
        Turn on
      </Button>
    </div>
  );
}
