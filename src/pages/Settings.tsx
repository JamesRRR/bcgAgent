import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Moon, RefreshCw, Sun } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useToaster } from "@/components/Toaster";
import { KB_KEYS, settings } from "@/lib/ipc";
import SecretField from "@/components/settings/SecretField";
import Section from "@/components/settings/Section";
import { getVersion } from "@tauri-apps/api/app";

const THEME_KEY = "theme";
const TTS_LANG_KEY = "tts_lang";
const RETRIEVAL_K_KEY = "retrieval_k";
const TTS_PROVIDER_KEY = "tts_provider";
const TTS_EL_VOICE_ID_KEY = "tts_elevenlabs_voice_id";

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
  const [brave, setBrave] = useState<SecretState>({
    initial: "",
    current: "",
  });
  const [savingKeys, setSavingKeys] = useState(false);

  // Wave 4 KB toggles — saved on change so there's no separate Save button.
  const [autoResearch, setAutoResearch] = useState<boolean>(true);
  const [includeUnofficial, setIncludeUnofficial] = useState<boolean>(true);
  const [confThreshold, setConfThreshold] = useState<number>(0.45);
  const [dailyCap, setDailyCap] = useState<number>(20);

  const [ttsLang, setTtsLang] = useState<"zh" | "en">("zh");
  const [retrievalK, setRetrievalK] = useState<number>(8);
  const [kError, setKError] = useState<string | null>(null);

  const [elevenKey, setElevenKey] = useState<SecretState>({
    initial: "",
    current: "",
  });
  const [elevenVoiceId, setElevenVoiceId] = useState<string>("");
  const [elevenVoiceIdInitial, setElevenVoiceIdInitial] = useState<string>("");
  type ProviderMode = "" | "elevenlabs" | "system";
  const [providerMode, setProviderMode] = useState<ProviderMode>("");

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
      settings.getSecret("elevenlabs"),
      settings.get(TTS_PROVIDER_KEY),
      settings.get(TTS_EL_VOICE_ID_KEY),
      settings.getSecret("brave"),
      settings.get(KB_KEYS.AUTO_RESEARCH),
      settings.get(KB_KEYS.INCLUDE_UNOFFICIAL),
      settings.get(KB_KEYS.CONFIDENCE_THRESHOLD),
      settings.get(KB_KEYS.RESEARCH_DAILY_CAP),
    ])
      .then(
        ([
          ds,
          mm,
          lang,
          k,
          el,
          prov,
          vid,
          br,
          autoR,
          incUn,
          conf,
          cap,
        ]) => {
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
          const elVal = el ?? "";
          setElevenKey({ initial: elVal, current: elVal });
          const vidVal = vid ?? "";
          setElevenVoiceId(vidVal);
          setElevenVoiceIdInitial(vidVal);
          const provNorm: ProviderMode =
            prov === "elevenlabs" || prov === "system" ? prov : "";
          setProviderMode(provNorm);

          // Wave 4 KB settings.
          const brVal = br ?? "";
          setBrave({ initial: brVal, current: brVal });
          if (autoR != null) setAutoResearch(autoR === "true" || autoR === "1");
          if (incUn != null)
            setIncludeUnofficial(incUn === "true" || incUn === "1");
          if (conf != null) {
            const n = parseFloat(conf);
            if (!Number.isNaN(n)) setConfThreshold(Math.min(1, Math.max(0, n)));
          }
          if (cap != null) {
            const n = parseInt(cap, 10);
            if (!Number.isNaN(n) && n >= 1 && n <= 200) setDailyCap(n);
          }
        },
      )
      .catch((e) => toaster.push(String(e), "error"));
    return () => {
      cancelled = true;
    };
  }, [toaster]);

  const dsDirty = dashscope.current !== dashscope.initial;
  const mmDirty = minimax.current !== minimax.initial;
  const elDirty = elevenKey.current !== elevenKey.initial;
  const elVidDirty = elevenVoiceId !== elevenVoiceIdInitial;
  const brDirty = brave.current !== brave.initial;
  const keysDirty = dsDirty || mmDirty || elDirty || elVidDirty || brDirty;

  const handleSaveKeys = async () => {
    setSavingKeys(true);
    try {
      const ops: Promise<unknown>[] = [];
      if (dsDirty)
        ops.push(settings.setSecret("dashscope", dashscope.current));
      if (mmDirty) ops.push(settings.setSecret("minimax", minimax.current));
      if (elDirty)
        ops.push(settings.setSecret("elevenlabs", elevenKey.current));
      if (elVidDirty)
        ops.push(settings.set(TTS_EL_VOICE_ID_KEY, elevenVoiceId));
      if (brDirty) ops.push(settings.setSecret("brave", brave.current));
      await Promise.all(ops);
      setDashscope((s) => ({ initial: s.current, current: s.current }));
      setMinimax((s) => ({ initial: s.current, current: s.current }));
      setElevenKey((s) => ({ initial: s.current, current: s.current }));
      setBrave((s) => ({ initial: s.current, current: s.current }));
      setElevenVoiceIdInitial(elevenVoiceId);
      toaster.push(isZh ? "已保存" : "Saved", "success");
    } catch (e) {
      toaster.push(String(e), "error");
    } finally {
      setSavingKeys(false);
    }
  };

  // Wave 4 KB persistance — bound to onChange/onBlur (no Save button).
  const persistKb = async (key: string, value: string) => {
    try {
      await settings.set(key, value);
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const handleProviderChange = async (next: ProviderMode) => {
    setProviderMode(next);
    try {
      await settings.set(TTS_PROVIDER_KEY, next);
    } catch (e) {
      toaster.push(String(e), "error");
    }
  };

  const activeLabel = ((): string => {
    const hasKey = !!elevenKey.initial;
    if (providerMode === "system") {
      return isZh ? "系统语音 (say)" : "System (say)";
    }
    if (providerMode === "elevenlabs") {
      return hasKey
        ? isZh ? "ElevenLabs（你的克隆音色）" : "ElevenLabs (your cloned voice)"
        : isZh ? "系统语音 (say) — 因密钥缺失自动回退" : "System (say) — fallback (no key)";
    }
    // Auto
    return hasKey
      ? isZh ? "ElevenLabs（你的克隆音色，自动）" : "ElevenLabs (your cloned voice, auto)"
      : isZh ? "系统语音 (say)" : "System (say)";
  })();

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

      <Section title={t("settings.kb.label")}>
        <SecretField
          label={t("settings.kb.braveKey")}
          value={brave.current}
          saved={!!brave.initial && !brDirty}
          dirty={brDirty}
          onChange={(v) => setBrave((s) => ({ ...s, current: v }))}
          helperText={emptyHelp}
          savedLabel={savedLabel}
          unsavedLabel={unsavedLabel}
        />
        <label className="flex items-center justify-between py-2 text-sm">
          <span>{t("settings.kb.autoResearch")}</span>
          <input
            type="checkbox"
            checked={autoResearch}
            onChange={(e) => {
              setAutoResearch(e.target.checked);
              persistKb(KB_KEYS.AUTO_RESEARCH, e.target.checked ? "true" : "false");
            }}
            className="accent-accent"
          />
        </label>
        <label className="flex items-center justify-between py-2 text-sm">
          <span>{t("settings.kb.includeUnofficial")}</span>
          <input
            type="checkbox"
            checked={includeUnofficial}
            onChange={(e) => {
              setIncludeUnofficial(e.target.checked);
              persistKb(
                KB_KEYS.INCLUDE_UNOFFICIAL,
                e.target.checked ? "true" : "false",
              );
            }}
            className="accent-accent"
          />
        </label>
        <div className="flex flex-col gap-1.5 py-2">
          <label className="text-sm text-ink/70 dark:text-cream/70">
            {t("settings.kb.dailyCap")}
          </label>
          <input
            type="number"
            min={1}
            max={200}
            step={1}
            value={dailyCap}
            onChange={(e) => {
              const n = parseInt(e.target.value, 10);
              if (!Number.isNaN(n)) setDailyCap(n);
            }}
            onBlur={() => {
              const clamped = Math.min(200, Math.max(1, dailyCap));
              setDailyCap(clamped);
              persistKb(KB_KEYS.RESEARCH_DAILY_CAP, String(clamped));
            }}
            className="w-32 rounded-md border border-ink/15 bg-paper px-3 py-2 text-sm text-ink outline-none focus:border-accent dark:bg-[var(--paper)] dark:text-cream dark:border-cream/15"
          />
        </div>
        <div className="flex flex-col gap-1.5 py-2">
          <label className="text-sm text-ink/70 dark:text-cream/70">
            {t("settings.kb.confThreshold")}{" "}
            <span className="text-ink/40">{confThreshold.toFixed(2)}</span>
          </label>
          <input
            type="range"
            min={0}
            max={1}
            step={0.05}
            value={confThreshold}
            onChange={(e) => setConfThreshold(parseFloat(e.target.value))}
            onMouseUp={() =>
              persistKb(KB_KEYS.CONFIDENCE_THRESHOLD, String(confThreshold))
            }
            onTouchEnd={() =>
              persistKb(KB_KEYS.CONFIDENCE_THRESHOLD, String(confThreshold))
            }
            className="accent-accent"
          />
        </div>
      </Section>

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

      <Section title={t("settings.elevenlabs.label")}>
        <p className="text-xs text-ink/50 dark:text-cream/50 mb-2">
          {t("settings.elevenlabs.intro")}
        </p>
        <SecretField
          label={t("settings.elevenlabs.apiKey")}
          value={elevenKey.current}
          saved={!!elevenKey.initial && !elDirty}
          dirty={elDirty}
          onChange={(v) => setElevenKey((s) => ({ ...s, current: v }))}
          helperText={emptyHelp}
          savedLabel={savedLabel}
          unsavedLabel={unsavedLabel}
        />
        <div className="flex flex-col gap-1.5">
          <label className="text-sm text-ink/70 dark:text-cream/70">
            {t("settings.elevenlabs.voiceId")}
          </label>
          <input
            type="text"
            value={elevenVoiceId}
            onChange={(e) => setElevenVoiceId(e.target.value)}
            placeholder={t("settings.elevenlabs.voiceIdPlaceholder")}
            className="w-full rounded-md border border-ink/15 bg-paper px-3 py-2 text-sm text-ink outline-none focus:border-accent dark:bg-[var(--paper)] dark:text-cream dark:border-cream/15"
          />
        </div>
        <div className="flex flex-col gap-1.5 pt-2">
          <label className="text-sm text-ink/70 dark:text-cream/70">
            {isZh ? "TTS 模式" : "TTS mode"}
          </label>
          <select
            value={providerMode}
            onChange={(e) =>
              handleProviderChange(e.target.value as ProviderMode)
            }
            className="w-full rounded-md border border-ink/15 bg-paper px-3 py-2 text-sm text-ink outline-none focus:border-accent dark:bg-[var(--paper)] dark:text-cream dark:border-cream/15"
          >
            <option value="">
              {isZh ? "自动（有密钥则用 ElevenLabs）" : "Auto (use ElevenLabs if key set)"}
            </option>
            <option value="elevenlabs">
              {isZh ? "强制 ElevenLabs" : "Force ElevenLabs"}
            </option>
            <option value="system">
              {isZh ? "强制系统语音 (say)" : "Force system voice (say)"}
            </option>
          </select>
          <p className="text-xs text-ink/40 dark:text-cream/40">
            {isZh ? "当前：" : "Active: "}{activeLabel}
          </p>
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

      <Section title={isZh ? "关于 / 更新" : "About / Updates"}>
        <UpdateRow isZh={isZh} />
      </Section>
    </section>
  );
}

function UpdateRow({ isZh }: { isZh: boolean }) {
  const [version, setVersion] = useState<string>("");
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);
  const triggerCheck = () => {
    window.dispatchEvent(new CustomEvent("bcg:check-for-update"));
  };
  return (
    <div className="flex items-center gap-4">
      <div className="text-sm text-ink/70 dark:text-cream/70">
        {isZh ? "当前版本" : "Current version"}: <span className="font-mono">v{version || "?"}</span>
      </div>
      <Button variant="outline" onClick={triggerCheck} className="gap-2">
        <RefreshCw className="w-4 h-4" />
        {isZh ? "检查更新" : "Check for updates"}
      </Button>
    </div>
  );
}
