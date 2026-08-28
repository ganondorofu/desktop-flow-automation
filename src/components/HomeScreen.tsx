import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FilePlus, FolderOpen, Clock, X } from "lucide-react";
import { forgetRecentFlow, listRecentFlows, type RecentFile } from "../data/fileOps";

interface HomeScreenProps {
  onNew: () => void;
  onOpen: () => void;
  onOpenRecent: (path: string) => void;
}

export function HomeScreen({ onNew, onOpen, onOpenRecent }: HomeScreenProps) {
  const { t } = useTranslation();
  const [recents, setRecents] = useState<RecentFile[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    listRecentFlows()
      .then(setRecents)
      .catch(() => setRecents([]))
      .finally(() => setLoaded(true));
  }, []);

  async function handleForget(event: React.MouseEvent, path: string) {
    event.stopPropagation();
    const next = await forgetRecentFlow(path).catch(() => null);
    if (next) setRecents(next);
  }

  return (
    <div className="home">
      <div className="home-hero">
        <span className="wordmark-mark home-mark" aria-hidden="true">
          <span />
          <span />
          <span />
          <span />
        </span>
        <h1>{t("app.wordmark")}</h1>
        <p>{t("home.tagline")}</p>
      </div>
      <div className="home-actions">
        <button className="home-action" onClick={onNew}>
          <FilePlus size={20} strokeWidth={1.7} aria-hidden="true" />
          {t("home.newFlow")}
        </button>
        <button className="home-action" onClick={onOpen}>
          <FolderOpen size={20} strokeWidth={1.7} aria-hidden="true" />
          {t("home.openFlow")}
        </button>
      </div>
      <div className="home-recent">
        <h2>{t("home.recent")}</h2>
        {loaded && recents.length === 0 && <p className="home-recent-empty">{t("home.recentEmpty")}</p>}
        {recents.length > 0 && (
          <ul>
            {recents.map((f) => (
              <li key={f.path}>
                <button className="home-recent-item" onClick={() => onOpenRecent(f.path)}>
                  <Clock size={13} strokeWidth={1.9} aria-hidden="true" />
                  <span className="home-recent-name">{f.name}</span>
                  <span className="home-recent-path">{f.path}</span>
                </button>
                <button
                  className="home-recent-forget"
                  onClick={(e) => handleForget(e, f.path)}
                  title={t("home.removeFromRecent")}
                  aria-label={t("home.removeFromRecent")}
                >
                  <X size={12} strokeWidth={2.25} aria-hidden="true" />
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
