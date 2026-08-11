import { lazy } from "react";

export const ThreadPage = lazy(async () => {
  const module = await import("@/ui/pages/ThreadPage");
  return { default: module.ThreadPage };
});

export const ProfilePage = lazy(async () => {
  const module = await import("@/ui/pages/ProfilePage");
  return { default: module.ProfilePage };
});

export const NotificationsPage = lazy(async () => {
  const module = await import("@/ui/pages/NotificationsPage");
  return { default: module.NotificationsPage };
});

export const SearchPage = lazy(async () => {
  const module = await import("@/ui/pages/SearchPage");
  return { default: module.SearchPage };
});

export const SettingsPage = lazy(async () => {
  const module = await import("@/ui/pages/SettingsPage");
  return { default: module.SettingsPage };
});
