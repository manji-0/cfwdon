import { useEffect, useState } from "react";
import { Announcement } from "@/domain/announcements/announcement";
import { dismissAnnouncement, fetchAnnouncements } from "@/infrastructure/api/announcements";

let cachedAnnouncements: ReadonlyArray<Announcement> | null = null;

export const resetAnnouncementBannerCache = (): void => {
  cachedAnnouncements = null;
};

export const AnnouncementBanner = () => {
  const [items, setItems] = useState<ReadonlyArray<Announcement>>(cachedAnnouncements ?? []);
  const unread = items.filter(Announcement.isUnread);

  useEffect(() => {
    if (cachedAnnouncements) {
      setItems(cachedAnnouncements);
      return;
    }
    let active = true;
    void fetchAnnouncements().then((result) => {
      if (!active || result.isErr()) {
        return;
      }
      cachedAnnouncements = result.value;
      setItems(result.value);
    });
    return () => {
      active = false;
    };
  }, []);

  const handleDismiss = async (id: string) => {
    const result = await dismissAnnouncement(id);
    if (result.isErr()) {
      return;
    }
    setItems((current) => {
      const next = current.map((item) => (item.id === id ? { ...item, read: true } : item));
      cachedAnnouncements = next;
      return next;
    });
  };

  if (unread.length === 0) {
    return null;
  }

  return (
    <div className="announcement-banner">
      {unread.map((item) => (
        <article key={item.id} className="app-card announcement-card">
          <div className="announcement-card-body" dangerouslySetInnerHTML={{ __html: item.content }} />
          <button
            type="button"
            className="app-button app-button-secondary"
            onClick={() => void handleDismiss(item.id)}
          >
            閉じる
          </button>
        </article>
      ))}
    </div>
  );
};
