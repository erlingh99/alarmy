
export interface Point {
    x: number;
    y: number;
}

export const getCurvePoints = (
    curve: "linear" | "asymptotic" | "s-curve" | "custom",
    startIntensity: number,
    endIntensity: number,
    hyperParameter?: number
): Point[] => {
    const points: Point[] = [];
    const steps = 100;

    const param = hyperParameter !== undefined ? hyperParameter :
        (curve === "s-curve" ? 10 : curve === "asymptotic" ? 4 : 0);

    for (let i = 0; i <= steps; i++) {
        const x = i / steps;
        let y: number;

        switch (curve) {
            case "asymptotic":
                if (param) {
                    y = startIntensity + (endIntensity - startIntensity) * (1 - Math.exp(-x * param * 0.4)) / (1 - Math.exp(-param * 0.4));
                } else {
                    y = startIntensity + (endIntensity - startIntensity) * x;
                }
                break;
            case "linear":
            default:
                y = startIntensity + (endIntensity - startIntensity) * x;
                break;
            case "s-curve":
                y = startIntensity + (endIntensity - startIntensity) * (Math.exp(param * x) - 1) / ((Math.exp(0.5 * param) - 1) * (1 + Math.exp(param * (x - 0.5))));
                break;
        }

        points.push({ x: x * 100, y });
    }

    return points;
};
