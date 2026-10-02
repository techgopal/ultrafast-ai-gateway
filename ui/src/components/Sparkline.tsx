const WIDTH = 120;
const HEIGHT = 32;
const PAD = 2;

interface SparklineProps {
  /** What the values are, as in "Requests per day". */
  label: string;
  values: readonly number[];
  /** How the lowest and the highest are said in the name. */
  format: (value: number) => string;
}

function pointsOf(values: readonly number[], low: number, high: number): string {
  const span = high - low;
  const step = values.length > 1 ? (WIDTH - 2 * PAD) / (values.length - 1) : 0;
  return values
    .map((value, index) => {
      const x = values.length > 1 ? PAD + index * step : WIDTH / 2;
      // A flat series runs through the middle.
      const y = span === 0 ? HEIGHT / 2 : HEIGHT - PAD - ((value - low) / span) * (HEIGHT - 2 * PAD);
      return `${Number(x.toFixed(2))},${Number(y.toFixed(2))}`;
    })
    .join(" ");
}

/**
 * A small line of the values, in order. Inline SVG in the colour of the text
 * around it (a class of the theme sets it), so it follows the theme. Its name
 * says the number of days and the lowest and the highest value, for those
 * who do not see it.
 */
export function Sparkline({ label, values, format }: SparklineProps) {
  if (values.length === 0) return null;
  const low = Math.min(...values);
  const high = Math.max(...values);
  const name = `${label}, ${values.length} days, lowest ${format(low)}, highest ${format(high)}`;
  return (
    <svg
      role="img"
      aria-label={name}
      viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
      preserveAspectRatio="none"
      focusable="false"
      className="h-8 w-full text-primary"
    >
      <title>{name}</title>
      <polyline
        points={pointsOf(values, low, high)}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}
