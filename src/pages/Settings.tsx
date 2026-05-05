import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Moon, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useToaster } from "@/components/Toaster";
import { settings } from "@/lib/ipc";
import SecretField from "@/components/settings/SecretField";
import Section from "@/components/settings/Section";

const THEME_KEY = "theme";
const TTS_LANG_KEY = "tts_lang";
const RETRIEVAL_K_KEY = "retrieval_k";

type SecretState = { initial: string; current: string };

export default function Settings() {
  const { t, i18n } = useTranslation();
  const toaster = useToaster();
  const isZh = i18n.language === "zh-CN";

  const [dashscope, setDashscope] = useState<SecretState>({
    initial: "",
    current: "",
  });
  const [minimax, setMinimax] = useState<SecretState>({
    initial: "",
    current: "",
  });
  const [savingKeys, setSavingKeys] = useState(false);

  const [ttsLang, setTtsLang] = useState<"zh" | "en">("zh");
  const [retrievalK, setRetrievalK] = useState<number>(8);
  const [kError, setKError] = useState<string | null>(null);

  const [dark, setDark] = useState<boolean>(
    () => document.documentElement.classList.contains("dark"),
  );

  // Load all settings on mount.
  useEffect(() => {
    let cancelled = false;
    Promise.all([
      settings.getSecret("dashscope"),
      settings.getSecret("minimax"),
      settings.get(TTS_LANG_KEY),
      settings.get(RETRIEVAL_K_KEY),
    ])
      .then(([ds, mm, lang, k]) => {
        if (cancelled) return;
        const dsVal = ds ?? "";
        const mmVal = mm ?? "";
        setDashscope({ initial: dsVal, current: dsVal });
        setMinimax({ initial: mmVal, current: mmVal });
        if (lang === "zh" || lang === "en") setTtsLang(lang);
        if (k) {
          const n = parseInt(k, 10);
          if (!Number.isNaN(n) && n >= 4 && n <= 16) setRetrievalK(n);
        }
      })
      .catch((e) => toaster.push(String(e), "error"));
    return () => {
      cancelled = true;
    };
  }, [toaster]);

  const dsDirty = dashscope.current !== dashscope.initial;
  const mmDirty = minimax.current !== minimax.initial;
  const keysDirty = dsDirty || mmDirty;

  const handleSaveKeys = async () => {
    setSavingKeys(true);
    try {
      const ops: Promise<unknown>[] = [];
      if (dsDirty)
        ops.push(settings.setSecret("dashscope", dashscope.current));
      if (mmDirty) ops.push(settings.setSecret("minimax", minimax.current));
      await Promise.all(ops);
      setDashscope((s) => ({ initial: s.current, current: s.current }));
      setMinimax((s) => ({ initial: s.current, current: s.current }));
      toaster.push(isZh ? "已保存" : "Saved", "success");
    } catch (e) {
      toaster.push(String(e), "error");
    } finally {
      setSavingKeys(false);
    }
  };

  const handleTtsLangChange = async (lang: "zh" | "en") => {
    setTtsLang(lang);
    try {
      await settings.set(TTS_LANG_KEY, lang);
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const handleKBlur = async () => {
    if (retrievalK < 4 || retrievalK > 16 || !Number.isInteger(retrievalK)) {
      setKError(isZh ? "请输入 4-16 的整数" : "Enter an integer 4-16");
      return;
    }
    setKError(null);
    try {
      await settings.set(RETRIEVAL_K_KEY, String(retrievalK));
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const toggleDark = () => {
    const next = !dark;
    setDark(next);
    if (next) {
      document.documentElement.classList.add("dark");
      localStorage.setItem(THEME_KEY, "dark");
    } else {
      document.documentElement.classList.remove("dark");
      localStorage.setItem(THEME_KEY, "light");
    }
  };

  const savedLabel = isZh ? "已保存" : "Saved";
  const unsavedLabel = isZh ? "未保存" : "Unsaved";
  const emptyHelp = isZh
    ? "留空并保存即可删除该密钥。"
    : "Save with an empty value to delete the key.";

  return (
    <section className="max-w-2xl mx-auto py-12 px-8 space-y-10 bg-cream dark:bg-[var(--bg)] text-ink dark:text-cream min-h-screen">
      <h1 className="text-3xl font-semibold">{t("settings.title")}</h1>

      <Section title={isZh ? "API 密钥" : "API Keys"}>
        <SecretField
          label={t("settings.secrets.dashscope")}
          value={dashscope.current}
          saved={!!dashscope.initial && !dsDirty}
          dirty={dsDirty}
          onChange={(v) => setDashscope((s) => ({ ...s, current: v }))}
          helperText={emptyHelp}
          savedLabel={savedLabel}
          unsavedLabel={unsavedLabel}
        />
        <SecretField
          label={t("settings.secrets.minimax")}
          value={minimax.current}
          saved={!!minimax.initial && !mmDirty}
          dirty={mmDirty}
          onChange={(v) => setMinimax((s) => ({ ...s, current: v }))}
          helperText={emptyHelp}
          savedLabel={savedLabel}
          unsavedLabel={unsavedLabel}
        />
        <div className="pt-2">
          <Button
            onClick={handleSaveKeys}
            disabled={!keysDirty || savingKeys}
          >
            {t("settings.secrets.save")}
          </Button>
        </div>
      </Section>

      <Section title={t("settings.voice.label")}>
        <div className="flex gap-6">
          {(["zh", "en"] as const).map((lang) => (
            <label
              key={lang}
              className="flex items-center gap-2 cursor-pointer text-sm"
            >
              <input
                type="radio"
                name="tts-lang"
                checked={ttsLang === lang}
                onChange={() => handleTtsLangChange(lang)}
                className="accent-accent"
              />
              {t(`settings.voice.${lang}`)}
            </label>
          ))}
        </div>
      </Section>

      <Section title={t("settings.retrieval.kLabel")}>
        <div className="flex flex-col gap-1.5">
          <input
            type="number"
            min={4}
            max={16}
            step={1}
            value={retrievalK}
            onChange={(e) => setRetrievalK(parseInt(e.target.value, 10))}
            onBlur={handleKBlur}
            className="w-32 rounded-md border border-ink/15 bg-paper px-3 py-2 text-sm text-ink outline-none focus:border-accent dark:bg-[var(--paper)] dark:text-cream dark:border-cream/15"
          />
          {kError && <p className="text-xs text-accent">{kError}</p>}
        </div>
      </Section>

      <Section title={isZh ? "外观" : "Appearance"}>
        <Button
          variant="outline"
          onClick={toggleDark}
          className="gap-2 dark:bg-[var(--paper)] dark:text-cream dark:border-cream/15 dark:hover:bg-[var(--bg)]"
        >
          {dark ? (
            <Sun className="h-4 w-4" />
          ) : (
            <Moon className="h-4 w-4" />
          )}
          {dark
            ? isZh
              ? "切换到浅色"
              : "Switch to light"
            : isZh
              ? "切换到深色"
              : "Switch to dark"}
        </Button>
      </Section>
    </section>
  );
}
