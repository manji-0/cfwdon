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

export const BookmarksPage = lazy(async () => {
  const module = await import("@/ui/pages/BookmarksPage");
  return { default: module.BookmarksPage };
});

export const ListsPage = lazy(async () => {
  const module = await import("@/ui/pages/ListsPage");
  return { default: module.ListsPage };
});

export const MessagesPage = lazy(async () => {
  const module = await import("@/ui/pages/MessagesPage");
  return { default: module.MessagesPage };
});

export const NewMessagePage = lazy(async () => {
  const module = await import("@/ui/pages/NewMessagePage");
  return { default: module.NewMessagePage };
});

export const ConversationPage = lazy(async () => {
  const module = await import("@/ui/pages/ConversationPage");
  return { default: module.ConversationPage };
});
