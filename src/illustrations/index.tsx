import type { SVGProps } from "react";

export function MeepleQuestion(props: SVGProps<SVGSVGElement>) {
  return (
    <svg viewBox="0 0 120 120" fill="none" xmlns="http://www.w3.org/2000/svg" {...props}>
      <path
        d="M60 18c-9 0-15 7-15 16 0 4 1 7 3 10-10 3-18 11-18 24v34h60V68c0-13-8-21-18-24 2-3 3-6 3-10 0-9-6-16-15-16Z"
        fill="var(--accent)"
      />
      <text
        x="60"
        y="78"
        textAnchor="middle"
        fontFamily="Caveat, cursive"
        fontSize="40"
        fill="var(--paper)"
      >
        ?
      </text>
    </svg>
  );
}

export function Dice(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 120 120"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    >
      <rect
        x="20"
        y="20"
        width="80"
        height="80"
        rx="14"
        fill="var(--paper)"
        stroke="var(--ink)"
        strokeWidth="3"
      />
      <circle cx="40" cy="40" r="6" fill="var(--accent)" />
      <circle cx="80" cy="40" r="6" fill="var(--ink)" />
      <circle cx="60" cy="60" r="6" fill="var(--accent)" />
      <circle cx="40" cy="80" r="6" fill="var(--ink)" />
      <circle cx="80" cy="80" r="6" fill="var(--accent)" />
    </svg>
  );
}

export function CardBack(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 120 120"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    >
      <rect
        x="28"
        y="14"
        width="64"
        height="92"
        rx="6"
        fill="var(--accent)"
        stroke="var(--ink)"
        strokeWidth="3"
      />
      <rect
        x="36"
        y="22"
        width="48"
        height="76"
        rx="3"
        fill="none"
        stroke="var(--paper)"
        strokeWidth="2"
        strokeDasharray="3 3"
      />
      <path
        d="M50 60 L60 44 L70 60 L60 76 Z"
        fill="var(--paper)"
      />
    </svg>
  );
}

export function MeepleMagnifier(props: SVGProps<SVGSVGElement>) {
  return (
    <svg
      viewBox="0 0 120 120"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      {...props}
    >
      <path
        d="M44 14c-7 0-12 5-12 12 0 3 1 6 3 8-8 2-14 9-14 19v27h46V53c0-10-6-17-14-19 2-2 3-5 3-8 0-7-5-12-12-12Z"
        fill="var(--shelf)"
      />
      <circle
        cx="82"
        cy="74"
        r="18"
        fill="var(--paper)"
        stroke="var(--ink)"
        strokeWidth="3"
      />
      <line
        x1="95"
        y1="87"
        x2="108"
        y2="100"
        stroke="var(--ink)"
        strokeWidth="5"
        strokeLinecap="round"
      />
    </svg>
  );
}
