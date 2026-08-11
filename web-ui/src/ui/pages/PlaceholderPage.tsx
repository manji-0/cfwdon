import { AppShell } from "@/ui/components/AppShell";

export const PlaceholderPage = ({ title, message }: Readonly<{ title: string; message: string }>) => (
  <AppShell title={title}>
    <div className="app-card">
      <p className="app-muted">{message}</p>
    </div>
  </AppShell>
);
