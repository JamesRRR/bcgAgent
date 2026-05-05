import type { ReactNode } from "react";

type Props = {
  title: string;
  children: ReactNode;
};

export default function Section({ title, children }: Props) {
  return (
    <section className="space-y-4">
      <h2 className="font-handwritten text-2xl text-ink/80 dark:text-cream/80">
        {title}
      </h2>
      <div className="space-y-4">{children}</div>
    </section>
  );
}
