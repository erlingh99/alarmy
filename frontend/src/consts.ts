export const TOAST_DURATION: number = 2000;
export const RESTORE_TOAST_DURATION: number = 3000;
export const INITIAL_ALARM_NAME: string = "";
export const ALARM_TOOLTIP: string = "Rise and shine"
export const INITIAL_ALARM_LENGTH: number = 15;
export const INITIAL_ALARM_TIME: string = "08:00";
export const INITIAL_START_INTENSITY: number = 0;
export const INITIAL_END_INTENSITY: number = 100;
export const INITIAL_INTENSITY_TYPE: "linear" | "asymptotic" | "s-curve" | "custom" = "linear";
export const INITIAL_ALARM_COLOR: string = "#4CAF50"
export const INITIAL_PLAY_MELODY: boolean = true;
export const DEFAULT_PARAM_S_CURVE: number = 10;
export const DEFAULT_PARAM_ASYMP: number = 10;
export const DEFAULT_INIT_CUSTOM: Array<{x: number, y: number}> = [{ x: 25, y: 40 }, { x: 75, y: 50 }]

// Pre-selected color options
export const presetColors: string[] = [
    "#F6E7D2", // Warm white
    "#FFDC52", // Light yellow 
    "#ffdba8", // Warmer white
    "#D946EF", // Magenta Pink
    "#F97316", // Bright Orange
    "#0EA5E9", // Ocean Blue
  ];
