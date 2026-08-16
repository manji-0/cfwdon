import { useEffect, useRef, useState } from "react";
import { mastodonErrorMessage } from "@/application/mastodon-error";
import type { AccountProfile } from "@/domain/account/account";
import { SessionState } from "@/domain/session/session";
import {
  fetchAccountCredentials,
  profileImageAccept,
  profileImageTooLarge,
  updateAccountProfile,
} from "@/infrastructure/api/credentials";
import { useSession } from "@/ui/context/SessionContext";

type ProfileEditorProps = Readonly<{
  profile: AccountProfile;
  onCancel: () => void;
  onSaved: (profile: AccountProfile) => void;
}>;

export const ProfileEditor = ({ profile, onCancel, onSaved }: ProfileEditorProps) => {
  const { session, setSession } = useSession();
  const avatarInputRef = useRef<HTMLInputElement>(null);
  const headerInputRef = useRef<HTMLInputElement>(null);
  const [displayName, setDisplayName] = useState(profile.displayName);
  const [note, setNote] = useState("");
  const [avatarFile, setAvatarFile] = useState<File | null>(null);
  const [headerFile, setHeaderFile] = useState<File | null>(null);
  const [avatarPreview, setAvatarPreview] = useState(profile.avatar);
  const [headerPreview, setHeaderPreview] = useState(profile.header);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");

  const avatarPreviewRef = useRef(avatarPreview);
  const headerPreviewRef = useRef(headerPreview);
  avatarPreviewRef.current = avatarPreview;
  headerPreviewRef.current = headerPreview;

  useEffect(() => {
    let active = true;
    void fetchAccountCredentials().then((result) => {
      if (!active || result.isErr()) {
        return;
      }
      setDisplayName(result.value.displayName);
      setNote(result.value.source.note);
    });
    return () => {
      active = false;
    };
  }, []);

  useEffect(
    () => () => {
      if (avatarPreviewRef.current.startsWith("blob:")) {
        URL.revokeObjectURL(avatarPreviewRef.current);
      }
      if (headerPreviewRef.current.startsWith("blob:")) {
        URL.revokeObjectURL(headerPreviewRef.current);
      }
    },
    [],
  );

  const assignImage = (
    file: File | undefined,
    setFile: (file: File | null) => void,
    currentPreview: string,
    setPreview: (url: string) => void,
  ) => {
    if (!file) {
      return;
    }
    if (!file.type.startsWith("image/") || profileImageTooLarge(file)) {
      setError("画像は 10MB 以下の image/* を選んでください");
      return;
    }
    setError("");
    if (currentPreview.startsWith("blob:")) {
      URL.revokeObjectURL(currentPreview);
    }
    setFile(file);
    setPreview(URL.createObjectURL(file));
  };

  const handleSave = async () => {
    setSaving(true);
    setError("");
    const result = await updateAccountProfile({
      displayName: displayName.trim(),
      note,
      avatar: avatarFile ?? undefined,
      header: headerFile ?? undefined,
    });
    if (result.isErr()) {
      setError(mastodonErrorMessage(result.error));
      setSaving(false);
      return;
    }
    const updated = result.value;
    if (session.kind === "Authenticated") {
      setSession(
        SessionState.updateAccount(session, {
          ...session.account,
          displayName: updated.displayName,
          avatar: updated.avatar,
        }),
      );
    }
    onSaved({
      ...profile,
      displayName: updated.displayName,
      note: updated.note,
      avatar: updated.avatar,
      header: updated.header || profile.header,
    });
    setSaving(false);
  };

  return (
    <form
      className="profile-editor"
      onSubmit={(event) => {
        event.preventDefault();
        void handleSave();
      }}
    >
      <input
        ref={headerInputRef}
        type="file"
        accept={profileImageAccept}
        hidden
        onChange={(event) => {
          assignImage(
            event.target.files?.[0],
            setHeaderFile,
            headerPreview,
            setHeaderPreview,
          );
          event.target.value = "";
        }}
      />
      <input
        ref={avatarInputRef}
        type="file"
        accept={profileImageAccept}
        hidden
        onChange={(event) => {
          assignImage(
            event.target.files?.[0],
            setAvatarFile,
            avatarPreview,
            setAvatarPreview,
          );
          event.target.value = "";
        }}
      />
      <button
        type="button"
        className="profile-header-banner profile-media-edit"
        style={{ backgroundImage: headerPreview ? `url(${headerPreview})` : undefined }}
        onClick={() => headerInputRef.current?.click()}
        disabled={saving}
      >
        <span className="profile-media-edit-label">背景を変更</span>
      </button>
      <div className="profile-header-body">
        <button
          type="button"
          className="profile-media-edit profile-avatar-edit"
          onClick={() => avatarInputRef.current?.click()}
          disabled={saving}
        >
          <img className="profile-avatar" src={avatarPreview} alt="" />
          <span className="profile-media-edit-label">アイコンを変更</span>
        </button>
        <label className="settings-field">
          <span>表示名</span>
          <input
            value={displayName}
            onChange={(event) => setDisplayName(event.target.value)}
            disabled={saving}
          />
        </label>
        <label className="settings-field">
          <span>自己紹介</span>
          <textarea
            value={note}
            onChange={(event) => setNote(event.target.value)}
            rows={4}
            disabled={saving}
          />
        </label>
        {error ? <p className="app-error">{error}</p> : null}
        <div className="profile-editor-actions">
          <button type="submit" className="app-button" disabled={saving}>
            {saving ? "保存中…" : "保存"}
          </button>
          <button
            type="button"
            className="app-button app-button-secondary"
            onClick={onCancel}
            disabled={saving}
          >
            キャンセル
          </button>
        </div>
      </div>
    </form>
  );
};
