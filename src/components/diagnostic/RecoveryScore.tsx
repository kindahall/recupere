interface RecoveryScoreProps {
  score: number; // 0-100
  label?: string;
  size?: number; // diameter in px, default 120
}

function getScoreColor(score: number): string {
  if (score >= 75) return 'var(--color-success)';
  if (score >= 50) return 'var(--color-warning)';
  if (score >= 25) return '#F97316'; // Orange
  return 'var(--color-danger)';
}

export function RecoveryScore({ score, label, size = 120 }: RecoveryScoreProps) {
  const radius = (size - 16) / 2; // account for stroke width
  const circumference = 2 * Math.PI * radius;
  const offset = circumference - (score / 100) * circumference;
  const center = size / 2;
  const color = getScoreColor(score);
  const accessibleLabel = label ? `${label}: ${score}%` : `Recovery score ${score}%`;

  return (
    <div className="recovery-score">
      <div className="recovery-score-ring" style={{ width: size, height: size }}>
        <svg width={size} height={size} role="img" aria-label={accessibleLabel}>
          <circle className="ring-bg" cx={center} cy={center} r={radius} />
          <circle
            className="ring-fill"
            cx={center}
            cy={center}
            r={radius}
            stroke={color}
            strokeDasharray={circumference}
            strokeDashoffset={offset}
          />
        </svg>
        <div className="recovery-score-value">
          {score}
          <span style={{ fontSize: '0.5em', opacity: 0.6 }}>%</span>
        </div>
      </div>
      {label && <div className="recovery-score-label">{label}</div>}
    </div>
  );
}
