import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Tributary",
  description: "Small streams. Pooled power.",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
