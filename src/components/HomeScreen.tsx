import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { FilePlus, FolderOpen, Clock, X, ChevronRight } from "lucide-react";
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
      <div className="home-shell">
        <header className="home-hero">
          <svg className="wordmark-mark home-mark" viewBox="0 0 24 24" aria-hidden="true">
            <path d="M6 6.5C9 6.5 10 12 12 12C14 12 15 17.5 18 17.5" className="wordmark-mark-path" fill="none" strokeWidth="2.4" strokeLinecap="round" />
            <circle cx="6" cy="6.5" r="2.4" className="wordmark-mark-node" />
            <circle cx="18" cy="17.5" r="2.4" className="wordmark-mark-node" />
            <circle cx="12" cy="12" r="3.6" className="wordmark-mark-hub" />
          </svg>
          <div>
            <h1>{t("app.wordmark")}</h1>
            <p>{t("home.tagline")}</p>
          </div>
        </header>

        <main className="home-workspace">
          <div className="home-actions">
            <button className="home-action home-action-primary" onClick={onNew}>
              <span className="home-action-icon"><FilePlus size={18} strokeWidth={1.9} aria-hidden="true" /></span>
              <span>{t("home.newFlow")}</span>
              <ChevronRight size={15} strokeWidth={2} aria-hidden="true" />
            </button>
            <button className="home-action" onClick={onOpen}>
              <span className="home-action-icon"><FolderOpen size={18} strokeWidth={1.9} aria-hidden="true" /></span>
              <span>{t("home.openFlow")}</span>
              <ChevronRight size={15} strokeWidth={2} aria-hidden="true" />
            </button>
          </div>

          <section className="home-recent">
            <h2>{t("home.recent")}</h2>
            {loaded && recents.length === 0 && <p className="home-recent-empty">{t("home.recentEmpty")}</p>}
            {recents.length > 0 && (
              <ul>
                {recents.map((f) => (
                  <li key={f.path}>
                    <button className="home-recent-item" onClick={() => onOpenRecent(f.path)}>
                      <Clock size={14} strokeWidth={1.9} aria-hidden="true" />
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
          </section>
        </main>
      </div>
    </div>
  );
}
