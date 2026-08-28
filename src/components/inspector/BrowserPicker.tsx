import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";

interface BrowserInfo {
  id: string;
  name: string;
  path: string;
}

/** `LaunchBrowser`'s browser choice — populated from whatever
 *  Chromium-family browsers `list_installed_browsers` actually found
 *  on this machine, so the dropdown always reflects reality instead
 *  of listing options that might not be installed. Empty selection
 *  means "whichever's found first", the same fallback the backend
 *  itself applies. */
export function BrowserPicker({ value, onChange }: { value: string; onChange: (browser: string) => void }) {
  const { t } = useTranslation();
  const [browsers, setBrowsers] = useState<BrowserInfo[] | null>(null);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    invoke<BrowserInfo[]>("list_installed_browsers")
      .then(setBrowsers)
      .catch(() => setBrowsers([]));
  }, []);

  return (
    <>
      <select className="num-input" value={value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{t("inspector.fields.browserAuto")}</option>
        {(browsers ?? []).map((b) => (
          <option key={b.id} value={b.id}>
            {b.name}
          </option>
        ))}
      </select>
      {browsers !== null && browsers.length === 0 && <p className="insp-hint insp-warning">{t("inspector.fields.browserNoneFound")}</p>}
    </>
  );
}
