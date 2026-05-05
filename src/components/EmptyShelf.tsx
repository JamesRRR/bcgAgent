import { useTranslation } from "react-i18next";
import { MeepleQuestion } from "@/illustrations";

export default function EmptyShelf() {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col items-center justify-center text-center py-16">
      <MeepleQuestion className="w-40 h-40 mb-6" />
      <p className="text-ink/70 font-handwritten text-2xl">
        {t("library.empty")}
      </p>
    </div>
  );
}
