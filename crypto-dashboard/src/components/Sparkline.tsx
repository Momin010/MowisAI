import { useMemo } from 'react';

interface SparklineProps {
  /** Array of numeric values to plot. */
  data: number[];
  /** SVG width in pixels. */
  width?: number;
  /** SVG height in pixels. */
  height?: number;
  /** Stroke and fill colour of the sparkline. */
  color?: string;
  /** Line thickness. */
  strokeWidth?: number;
  /** Whether to render a filled area beneath the line. */
  fill?: boolean;
  /** Opacity of the filled area (0-1). */
  fillOpacity?: number;
  /** Optional CSS class applied to the outer SVG element. */
  className?: string;
}

const Sparkline = ({
  data,
  width = 120,
  height = 32,
  color = '#111',
  strokeWidth = 1.5,
  fill = false,
  fillOpacity = 0.1,
  className,
}: SparklineProps) => {
  const { linePath, areaPath, viewBox } = useMemo(() => {
    if (data.length < 2) {
      return { linePath: '', areaPath: '', viewBox: `0 0 ${width} ${height}` };
    }

    const min = Math.min(...data);
    const max = Math.max(...data);
    const range = max - min || 1;

    // Horizontal / vertical padding so the stroke never clips at the edges.
    const padX = strokeWidth;
    const padY = strokeWidth;
    const plotW = width - padX * 2;
    const plotH = height - padY * 2;

    const points = data.map((value, index) => ({
      x: padX + (index / (data.length - 1)) * plotW,
      y: padY + plotH - ((value - min) / range) * plotH,
    }));

    const line = points
      .map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(2)},${p.y.toFixed(2)}`)
      .join(' ');

    const area = `${line} L${points[points.length - 1].x.toFixed(2)},${(height - padY).toFixed(2)} L${points[0].x.toFixed(2)},${(height - padY).toFixed(2)} Z`;

    return {
      linePath: line,
      areaPath: area,
      viewBox: `0 0 ${width} ${height}`,
    };
  }, [data, width, height, strokeWidth]);

  if (data.length < 2) {
    return null;
  }

  return (
    <svg
      width={width}
      height={height}
      viewBox={viewBox}
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      role="img"
      aria-label="Sparkline chart"
    >
      {fill && (
        <path
          d={areaPath}
          fill={color}
          fillOpacity={fillOpacity}
          stroke="none"
        />
      )}
      <path
        d={linePath}
        stroke={color}
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
};

export default Sparkline;
