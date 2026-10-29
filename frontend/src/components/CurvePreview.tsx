import { type RefObject, type PointerEvent } from "react";
import { Point } from "@/lib/curveUtils";
import { cn } from "@/lib/utils";

const ENDPOINT_RADIUS = 4;
const CONTROL_POINT_RADIUS = 6;

interface CurvePreviewProps {
    svgRef: RefObject<SVGSVGElement | null>;
    svgWidth: number;
    svgHeight: number;
    padding: number;
    pathCommand: string;
    startIntensity: number;
    endIntensity: number;
    curveType: string;
    controlPoints: Point[];
    activePointIndex: number | null;
    onPointerDown: (event: PointerEvent<SVGSVGElement>) => void;
    onPointerMove: (event: PointerEvent<SVGSVGElement>) => void;
    onPointerUp: (event: PointerEvent<SVGSVGElement>) => void;
}

export function CurvePreview({
    svgRef,
    svgWidth,
    svgHeight,
    padding,
    pathCommand,
    startIntensity,
    endIntensity,
    curveType,
    controlPoints,
    activePointIndex,
    onPointerDown,
    onPointerMove,
    onPointerUp,
}: CurvePreviewProps) {
    return (
        <div className="border rounded-md p-4 bg-background/50">
            <svg
                ref={svgRef}
                width={svgWidth}
                height={svgHeight}
                viewBox={`0 0 ${svgWidth} ${svgHeight}`}
                className={cn(
                    "w-full touch-none",
                    curveType === "custom" && "cursor-pointer"
                )}
                onPointerDown={onPointerDown}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
                onPointerCancel={onPointerUp}
            >
                {/* Grid lines */}
                <line
                    x1={padding / 2}
                    y1={svgHeight - padding / 2}
                    x2={svgWidth - padding / 2}
                    y2={svgHeight - padding / 2}
                    stroke="currentColor"
                    strokeWidth="1"
                    className="text-muted/30"
                />
                <line
                    x1={padding / 2}
                    y1={padding / 2}
                    x2={padding / 2}
                    y2={svgHeight - padding / 2}
                    stroke="currentColor"
                    strokeWidth="1"
                    className="text-muted/30"
                />

                {/* Curve path */}
                <path
                    d={pathCommand}
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2.5"
                    className="text-primary hover:text-primary/80 transition-colors"
                />

                {/* Start point */}
                <circle
                    cx={padding / 2}
                    cy={(1 - startIntensity / 100) * (svgHeight - padding) + padding / 2}
                    r={ENDPOINT_RADIUS}
                    fill="currentColor"
                    className="text-primary"
                />

                {/* End point */}
                <circle
                    cx={svgWidth - padding / 2}
                    cy={(1 - endIntensity / 100) * (svgHeight - padding) + padding / 2}
                    r={ENDPOINT_RADIUS}
                    fill="currentColor"
                    className="text-primary"
                />

                {/* Control points for custom curve */}
                {curveType === "custom" && controlPoints.map((point, index) => (
                    <circle
                        key={index}
                        cx={(point.x / 100) * (svgWidth - padding) + padding / 2}
                        cy={(1 - point.y / 100) * (svgHeight - padding) + padding / 2}
                        r={CONTROL_POINT_RADIUS}
                        fill={activePointIndex === index ? "currentColor" : "white"}
                        stroke="currentColor"
                        strokeWidth="2"
                        className={cn(
                            "cursor-move transition-transform hover:scale-125 [transform-box:fill-box] [transform-origin:center]",
                            activePointIndex === index ? "text-primary/80" : "text-primary"
                        )}
                    />
                ))}
            </svg>
            <div className="flex justify-between text-xs text-muted-foreground mt-2 font-medium">
                <span>0%</span>
                <span className="uppercase tracking-wider">Time Progression</span>
                <span>100%</span>
            </div>
        </div>
    );
}
