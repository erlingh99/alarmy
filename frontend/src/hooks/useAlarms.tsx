
import { useQuery } from "@tanstack/react-query";
import { alarmService } from "@/services/alarmService";
import { toast } from "sonner";
import { TOAST_DURATION } from "@/consts";

export function useAlarms() {
  const { 
    data: alarms = [], 
    isLoading, 
    isError, 
    error 
  } = useQuery({
    queryKey: ["alarms"],
    queryFn: alarmService.getAlarms,
    meta: {
      onError: (error: Error) => {
        console.error("Error fetching alarms:", error);
        toast.error(`Failed to load alarms: ${error.message}`, {
          duration: TOAST_DURATION,
        });
      },
    },
  });

  return { 
    alarms, 
    isLoading, 
    isError, 
    error 
  };
}
