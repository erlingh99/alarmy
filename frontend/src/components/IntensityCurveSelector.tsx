
import { useState, useRef, useEffect, useMemo, type PointerEvent } from "react";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { IntensityCurve } from "@/types/alarm";
import { ChevronDown, ChevronUp } from "lucide-react";
import { cn } from "@/lib/utils";
import { DEFAULT_INIT_CUSTOM, DEFAULT_PARAM_ASYMP, DEFAULT_PARAM_S_CURVE } from "@/consts";
import { getCurvePoints, Point } from "@/lib/curveUtils";
import { CurvePreview } from "./CurvePreview";
import { toast } from "sonner";

const LONG_PRESS_MS = 600;
const MAX_CONTROL_POINTS = 5;
const POINT_HIT_RADIUS = 10;
const DRAG_THRESHOLD = 5;
const SVG_HEIGHT = 120;
const SVG_DEFAULT_WIDTH = 400;
const SVG_PADDING = 10;

interface IntensityCurveSelectorProps {
  value: IntensityCurve;
  onChange: (curve: IntensityCurve) => void;
}

function getCatmullRomPoints(
  controlPoints: Point[],
  startIntensity: number,
  endIntensity: number
): Point[] {
  const pts: Point[] = [
    { x: 0, y: startIntensity },
    ...controlPoints,
    { x: 100, y: endIntensity },
  ];

  const result: Point[] = [];
  for (let i = 0; i <= 100; i++) {
    const x = i;
    let segIdx = pts.length - 2;
    for (let j = 0; j < pts.length - 1; j++) {
      if (x <= pts[j + 1].x) { segIdx = j; break; }
    }
    const p1 = pts[segIdx];
    const p2 = pts[segIdx + 1];
    const p0 = segIdx > 0 ? pts[segIdx - 1] : p1;
    const p3 = segIdx + 2 < pts.length ? pts[segIdx + 2] : p2;
    const dx = p2.x - p1.x;
    if (dx < 1e-6) { result.push({ x, y: Math.max(0, Math.min(100, p1.y)) }); continue; }
    const t = (x - p1.x) / dx;
    const t2 = t * t;
    const t3 = t2 * t;
    const y = 0.5 * (
      2 * p1.y +
      (-p0.y + p2.y) * t +
      (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 +
      (-p0.y + 3 * p1.y - 3 * p2.y + p3.y) * t3
    );
    result.push({ x, y: Math.max(0, Math.min(100, y)) });
  }
  return result;
}

export function IntensityCurveSelector({ value, onChange }: IntensityCurveSelectorProps) {
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [isDragging, setIsDragging] = useState(false);
  const [activePointIndex, setActivePointIndex] = useState<number | null>(null);
  const [containerWidth, setContainerWidth] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);

  const handleStartIntensityChange = (values: number[]) => {
    onChange({ ...value, start_intensity: values[0] });
  };

  const handleEndIntensityChange = (values: number[]) => {
    onChange({ ...value, end_intensity: values[0] });
  };

  const handleCurveTypeChange = (curveType: "linear" | "s-curve" | "asymptotic" | "custom") => {
    let hyperParameter: number | undefined;
    let initialPoints: Point[] | undefined;

    if (curveType === "s-curve") hyperParameter = DEFAULT_PARAM_S_CURVE;
    else if (curveType === "asymptotic") hyperParameter = DEFAULT_PARAM_ASYMP;
    else if (curveType === "custom") initialPoints = DEFAULT_INIT_CUSTOM as Point[];

    onChange({
      ...value,
      curve: curveType,
      hyper_parameter: hyperParameter,
      control_points: initialPoints,
    });
  };

  const handleHyperParameterChange = (newValue: number) => {
    onChange({ ...value, hyper_parameter: newValue });
  };

  const getHyperParameterInfo = () => {
    switch (value.curve) {
      case "s-curve":
        return { name: "Sharpness", description: "Controls the S-curve transition (higher = sharper)", min: 1, max: 50, step: 1, default: 10, disabled: false };
      case "asymptotic":
        return { name: "Decay Rate", description: "Controls how quickly the curve approaches target", min: -50, max: 50, step: 1, default: 10, disabled: false };
      case "custom":
        return { name: "Curve Editing", description: "Drag points to create a custom shape", min: 0, max: 1, step: 0.1, default: 0, disabled: true };
      default:
        return { name: "Parameter", description: "No adjustable parameters for this curve", min: 0, max: 1, step: 0.1, default: 0, disabled: true };
    }
  };

  const hyperParam = getHyperParameterInfo();

  useEffect(() => {
    if (!containerRef.current) return;
    const observer = new ResizeObserver(entries => {
      for (const entry of entries) setContainerWidth(entry.contentRect.width);
    });
    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, []);

  const getSvgCoordinates = (clientX: number, clientY: number) => {
    if (!svgRef.current) return { x: 0, y: 0 };
    const svgRect = svgRef.current.getBoundingClientRect();
    const x = ((clientX - svgRect.left) / (svgRect.right - svgRect.left)) * 100;
    const y = 100 - ((clientY - svgRect.top) / (svgRect.bottom - svgRect.top)) * 100;
    return { x: Math.max(1, Math.min(99, x)), y: Math.max(0, Math.min(100, y)) };
  };

  const [activePointerId, setActivePointerId] = useState<number | null>(null);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const dragStartCoords = useRef<{ x: number; y: number } | null>(null);
  const activePointIndexRef = useRef<number | null>(null);
  const workingPointsRef = useRef<Point[] | null>(null);

  const cancelLongPress = () => {
    if (longPressTimer.current !== null) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  };

  const handlePointerStart = (event: PointerEvent<SVGSVGElement>) => {
    if (value.curve !== "custom") return;
    const coords = getSvgCoordinates(event.clientX, event.clientY);
    const controlPoints = (value.control_points || []) as Point[];

    const pointIndex = controlPoints.findIndex(point => {
      const distance = Math.sqrt(Math.pow(point.x - coords.x, 2) + Math.pow(point.y - coords.y, 2));
      return distance < POINT_HIT_RADIUS;
    });

    if (pointIndex >= 0) {
      dragStartCoords.current = coords;
      activePointIndexRef.current = pointIndex;
      workingPointsRef.current = [...controlPoints];
      longPressTimer.current = setTimeout(() => {
        const newPoints = [...controlPoints];
        newPoints.splice(pointIndex, 1);
        onChange({ ...value, control_points: newPoints });
        setIsDragging(false);
        setActivePointIndex(null);
        setActivePointerId(null);
        activePointIndexRef.current = null;
        workingPointsRef.current = null;
      }, LONG_PRESS_MS);

      if (event.isPrimary) {
        setActivePointIndex(pointIndex);
        setIsDragging(true);
        setActivePointerId(event.pointerId);
        event.currentTarget.setPointerCapture(event.pointerId);
      }
    } else if (controlPoints.length < MAX_CONTROL_POINTS) {
      const newPoints = [...controlPoints, coords].sort((a, b) => a.x - b.x);
      onChange({ ...value, control_points: newPoints });
    } else {
      toast.info(`Maximum of ${MAX_CONTROL_POINTS} control points reached`);
    }
  };

  const handlePointerMove = (event: PointerEvent<SVGSVGElement>) => {
    const currentIndex = activePointIndexRef.current;
    if (!isDragging || currentIndex === null || workingPointsRef.current === null || value.curve !== "custom" || event.pointerId !== activePointerId) return;
    const coords = getSvgCoordinates(event.clientX, event.clientY);

    if (dragStartCoords.current) {
      const dx = coords.x - dragStartCoords.current.x;
      const dy = coords.y - dragStartCoords.current.y;
      if (Math.sqrt(dx * dx + dy * dy) > DRAG_THRESHOLD) {
        cancelLongPress();
      }
    }

    const points = [...workingPointsRef.current];
    points[currentIndex] = coords;
    const sorted = points.sort((a, b) => a.x - b.x);
    const newIndex = sorted.indexOf(coords);
    activePointIndexRef.current = newIndex;
    workingPointsRef.current = sorted;
    if (newIndex !== currentIndex) setActivePointIndex(newIndex);
    onChange({ ...value, control_points: sorted });
  };

  const handlePointerUp = (event: PointerEvent<SVGSVGElement>) => {
    cancelLongPress();
    if (event.pointerId === activePointerId) {
      setIsDragging(false);
      setActivePointIndex(null);
      setActivePointerId(null);
      activePointIndexRef.current = null;
      workingPointsRef.current = null;
      dragStartCoords.current = null;
      try { event.currentTarget.releasePointerCapture(event.pointerId); } catch {
        // Pointer capture might have been lost already
      }
    }
  };

  const svgWidth = containerWidth || SVG_DEFAULT_WIDTH;

  const curvePoints = useMemo(
    () => value.curve === "custom"
      ? getCatmullRomPoints((value.control_points || []) as Point[], value.start_intensity, value.end_intensity)
      : getCurvePoints(value.curve, value.start_intensity, value.end_intensity, value.hyper_parameter),
    [value.curve, value.start_intensity, value.end_intensity, value.hyper_parameter, value.control_points]
  );

  const pathCommand = curvePoints.map((p, i) => {
    const x = (p.x / 100) * (svgWidth - SVG_PADDING) + SVG_PADDING / 2;
    const y = (1 - p.y / 100) * (SVG_HEIGHT - SVG_PADDING) + SVG_PADDING / 2;
    return `${i === 0 ? 'M' : 'L'} ${x} ${y}`;
  }).join(' ');

  return (
    <div className="space-y-4">
      <div>
        <Label>Intensity Curve</Label>
        <Select
          value={value.curve}
          onValueChange={(val) => handleCurveTypeChange(val as IntensityCurve['curve'])}
        >
          <SelectTrigger><SelectValue placeholder="Select curve type" /></SelectTrigger>
          <SelectContent>
            <SelectItem value="linear">Linear</SelectItem>
            <SelectItem value="s-curve">S-Curve</SelectItem>
            <SelectItem value="asymptotic">Asymptotic</SelectItem>
            <SelectItem value="custom">Custom</SelectItem>
          </SelectContent>
        </Select>
      </div>

      <button
        onClick={() => setShowAdvanced(!showAdvanced)}
        type="button"
        className="flex items-center text-sm font-medium text-muted-foreground hover:text-foreground transition-colors"
      >
        {showAdvanced ? <ChevronUp className="h-4 w-4 mr-1" /> : <ChevronDown className="h-4 w-4 mr-1" />}
        Advanced Settings
      </button>

      {showAdvanced && (
        <div className="pl-4 py-2 border-l-2 border-primary/20 space-y-4">
          <div className="space-y-2">
            <Label className="text-xs">Start Intensity: {value.start_intensity}%</Label>
            <Slider value={[value.start_intensity]} min={0} max={100} step={1} onValueChange={handleStartIntensityChange} />
          </div>
          <div className="space-y-2">
            <Label className="text-xs">End Intensity: {value.end_intensity}%</Label>
            <Slider value={[value.end_intensity]} min={0} max={100} step={1} onValueChange={handleEndIntensityChange} />
          </div>
          <div className="space-y-2">
            <Label className={cn("text-xs", hyperParam.disabled && "text-muted-foreground")}>
              {hyperParam.name} {!hyperParam.disabled && value.hyper_parameter !== undefined && `: ${value.hyper_parameter.toFixed(0)}`}
            </Label>
            <p className="text-[10px] text-muted-foreground leading-none">{hyperParam.description}</p>
            {value.curve !== "custom" && (
              <Slider
                value={[value.hyper_parameter ?? hyperParam.default]}
                min={hyperParam.min}
                max={hyperParam.max}
                step={hyperParam.step}
                onValueChange={(vs) => handleHyperParameterChange(vs[0])}
                disabled={hyperParam.disabled}
              />
            )}
            {value.curve === "custom" && (
              <div className="text-[10px] text-primary/70 italic">
                • Tap graph to add points (max {MAX_CONTROL_POINTS}) • Drag to adjust • Long-press to remove
              </div>
            )}
          </div>
        </div>
      )}

      <div ref={containerRef}>
        <CurvePreview
          svgRef={svgRef}
          svgWidth={svgWidth}
          svgHeight={SVG_HEIGHT}
          padding={SVG_PADDING}
          pathCommand={pathCommand}
          startIntensity={value.start_intensity}
          endIntensity={value.end_intensity}
          curveType={value.curve}
          controlPoints={(value.control_points || []) as Point[]}
          activePointIndex={activePointIndex}
          onPointerDown={handlePointerStart}
          onPointerMove={handlePointerMove}
          onPointerUp={handlePointerUp}
        />
      </div>
    </div>
  );
}
