import { useState, useMemo } from 'react';
import { Menu, List, Cpu, Bell, BellOff, History } from 'lucide-react';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator,
} from '@/components/ui/dropdown-menu';
import { LogEntry } from '@/services/logService';
import { Button } from '@/components/ui/button';
import { ModeService, AlarmToggleEvent } from '@/services/modeService';
import { useAlarmsEnabled } from '@/hooks/useAlarmsEnabled';
import { LogService } from '@/services/logService';
import { FirmwareService, FirmwareVersion, formatVersion } from '@/services/firmwareService';
import { ColorPicker } from '@/components/ColorPicker';
import { toast } from "sonner";
import Convert from 'ansi-to-html';
import DOMPurify from 'dompurify';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";

const DEFAULT_LED_COLOR = '#ffffff';

export const LedStripMenu = () => {
  const [isColorPickerOpen, setIsColorPickerOpen] = useState(false);
  const [isLogsOpen, setIsLogsOpen] = useState(false);
  const [isVersionOpen, setIsVersionOpen] = useState(false);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [currentColor, setCurrentColor] = useState<string | null>(null);
  const [tempColor, setTempColor] = useState<string>(DEFAULT_LED_COLOR);
  const [backendVersion, setBackendVersion] = useState<FirmwareVersion | null>(null);
  const [deviceVersion, setDeviceVersion] = useState<FirmwareVersion | null>(null);
  const [versionLoading, setVersionLoading] = useState(false);
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [history, setHistory] = useState<AlarmToggleEvent[]>([]);
  const { alarmsEnabled, setAlarmsEnabled, isToggling } = useAlarmsEnabled();
  const modeService = ModeService.getInstance();
  const logService = LogService.getInstance();
  const firmwareService = FirmwareService.getInstance();
  const ansiConverter = useMemo(() => new Convert(), []);

  const handleColorChange = (color: string) => {
    setTempColor(color);
  };

  const applyColor = async (color: string) => {
    const promise = modeService.sendColor(color);

    toast.promise(promise, {
      loading: 'Setting color...',
      success: () => {
        setCurrentColor(color);
        return `Color set to ${color}`;
      },
      error: 'Failed to set color',
    });
  };

  const handleTurnOff = async () => {
    const promise = modeService.sendColor(null);

    toast.promise(promise, {
      loading: 'Turning off lights...',
      success: () => {
        setCurrentColor(null);
        return 'Lights turned off';
      },
      error: 'Failed to turn off lights',
    });
  };

  const handleShowLogs = async () => {
    try {
      const fetchedLogs = await logService.getLogs(30);
      setLogs(fetchedLogs);
      setIsLogsOpen(true);
    } catch {
      toast.error('Failed to fetch logs');
    }
  };

  const handleFireAnimation = async () => {
    const promise = modeService.sendFireAnimation();

    toast.promise(promise, {
      loading: 'Starting fire animation...',
      success: 'Fire animation started',
      error: 'Failed to start fire animation',
    });
  };

  const handleRainbowAnimation = async () => {
    const promise = modeService.sendRainbowAnimation();

    toast.promise(promise, {
      loading: 'Starting rainbow animation...',
      success: 'Rainbow animation started',
      error: 'Failed to start rainbow animation',
    });
  };

  const handleShowHistory = async () => {
    try {
      setHistory(await modeService.getToggleHistory(30));
      setIsHistoryOpen(true);
    } catch {
      toast.error('Failed to fetch alarm history');
    }
  };

  const handleShowVersion = async () => {
    setVersionLoading(true);
    setIsVersionOpen(true);
    try {
      const [backend, device] = await Promise.allSettled([
        firmwareService.getBackendVersion(),
        firmwareService.getDeviceVersion(),
      ]);
      setBackendVersion(backend.status === 'fulfilled' ? backend.value : null);
      setDeviceVersion(device.status === 'fulfilled' ? device.value : null);
    } finally {
      setVersionLoading(false);
    }
  };

  const handleColorPickerCancel = () => {
    setIsColorPickerOpen(false);
    setTempColor(currentColor ?? DEFAULT_LED_COLOR);
  };

  const handleColorPickerApply = () => {
    applyColor(tempColor);
    setIsColorPickerOpen(false);
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" className="rounded-full cursor-pointer">
          <Menu className="h-4 w-4" />
          <span className="sr-only">Open menu</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={() => setIsColorPickerOpen(true)} className='cursor-pointer'>
          Choose Color
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={handleFireAnimation} className='cursor-pointer'>
          Fire Animation
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={handleRainbowAnimation} className='cursor-pointer'>
          Rainbow Animation
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={handleTurnOff} className='cursor-pointer'>
          Turn Off Light
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem
          onSelect={() => setAlarmsEnabled(!alarmsEnabled)}
          disabled={isToggling}
          className='cursor-pointer'
        >
          {alarmsEnabled ? (
            <BellOff className="mr-2 h-4 w-4" />
          ) : (
            <Bell className="mr-2 h-4 w-4" />
          )}
          {alarmsEnabled ? 'Disable Alarms' : 'Enable Alarms'}
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={handleShowHistory} className='cursor-pointer'>
          <History className="mr-2 h-4 w-4" />
          Alarm History
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={handleShowLogs} className='cursor-pointer'>
          <List className="mr-2 h-4 w-4" />
          Show Logs
        </DropdownMenuItem>
        <DropdownMenuItem onSelect={handleShowVersion} className='cursor-pointer'>
          <Cpu className="mr-2 h-4 w-4" />
          Firmware Version
        </DropdownMenuItem>
      </DropdownMenuContent>

      <Dialog open={isLogsOpen} onOpenChange={setIsLogsOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Last 30 Logs</DialogTitle>
          </DialogHeader>
          <div className="max-h-[400px] overflow-y-auto">
            {logs.map((log) => (
              <div key={log.id} className="py-2 border-b last:border-b-0">
                <div className="text-sm text-muted-foreground">
                  {new Date(log.timestamp).toLocaleString()}
                </div>
                <div
                  className="mt-1"
                  dangerouslySetInnerHTML={{
                    __html: DOMPurify.sanitize(ansiConverter.toHtml(log.message))
                  }}
                />
              </div>
            ))}
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={isVersionOpen} onOpenChange={setIsVersionOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Firmware Version</DialogTitle>
          </DialogHeader>
          {versionLoading ? (
            <p className="text-sm text-muted-foreground">Loading...</p>
          ) : (
            <div className="space-y-3">
              <div className="flex justify-between items-center">
                <span className="text-sm font-medium">Backend firmware</span>
                <span className="text-sm font-mono">
                  {backendVersion ? formatVersion(backendVersion) : <span className="text-destructive">unavailable</span>}
                </span>
              </div>
              <div className="flex justify-between items-center">
                <span className="text-sm font-medium">Device running</span>
                <span className="text-sm font-mono">
                  {deviceVersion ? formatVersion(deviceVersion) : <span className="text-destructive">unavailable</span>}
                </span>
              </div>
            </div>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={isHistoryOpen} onOpenChange={setIsHistoryOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Alarm History</DialogTitle>
          </DialogHeader>
          {history.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No changes recorded yet. Alarms have been enabled the whole time.
            </p>
          ) : (
            <div className="max-h-[400px] overflow-y-auto">
              <p className="pb-2 text-sm text-muted-foreground">
                {history.length} change{history.length === 1 ? '' : 's'}, currently{' '}
                {history[0].enabled ? 'enabled' : 'disabled'}.
              </p>
              {history.map((event) => (
                <div
                  key={event.id}
                  className="flex justify-between items-center py-2 border-b last:border-b-0"
                >
                  <span className="text-sm font-medium">
                    {event.enabled ? 'Enabled' : 'Disabled'}
                  </span>
                  <span className="text-sm text-muted-foreground">
                    {new Date(event.changed_at).toLocaleString()}
                  </span>
                </div>
              ))}
            </div>
          )}
        </DialogContent>
      </Dialog>

      <Dialog open={isColorPickerOpen} onOpenChange={(open) => {
        if (!open) handleColorPickerCancel();
        else setIsColorPickerOpen(true);
      }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Choose a Color</DialogTitle>
          </DialogHeader>
          <ColorPicker value={tempColor} onChange={handleColorChange} />
          <div className="flex justify-end mt-4 gap-2">
            <Button variant="outline" onClick={handleColorPickerCancel}>
              Cancel
            </Button>
            <Button onClick={handleColorPickerApply}>
              Apply
            </Button>
          </div>
        </DialogContent>
      </Dialog>
    </DropdownMenu>
  );
};
