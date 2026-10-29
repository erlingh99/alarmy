
export type RecurrencePattern = {
  recurrence_type: "daily" | "weekly" | "custom";
  days?: [boolean, boolean, boolean, boolean, boolean, boolean, boolean];  // 0-6 for weekly (Monday-Sunday)
  custom_dates?: string[]; // For custom dates in ISO format
};

export type IntensityCurve = {
  start_intensity: number; // 0-100
  end_intensity: number; // 0-100
  curve: "linear" | "asymptotic" | "s-curve" | "custom"; // Type of curve
  hyper_parameter?: number; // Curve-specific parameter (e.g., sharpness for s-curve, decay rate for asymptotic)
  control_points?: Array<{x: number, y: number}>; // Control points for custom curve (x: 0-100, y: 0-100)
};

export type Alarm = {
  id: string;
  name: string;
  time: string;
  color: string; // Hex color code
  length: number; // Length in minutes
  intensity_curve: IntensityCurve;
  is_active: boolean;
  recurrence: RecurrencePattern;
  play_melody: boolean;
  created_at: string;
  updated_at: string;
};
