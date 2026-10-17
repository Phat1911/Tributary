import type { Metadata } from "next";
import { Space_Grotesk } from "next/font/google";
import { Toaster } from "@/components/ui/sonner";
import "./globals.css";

// Display font for headings — the one deliberate typographic voice; body
// stays on the system stack (fast, native, no extra download).
const display = Space_Grotesk({
  subsets: ["latin"],
  variable: "--font-display",
});

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
    <html lang="en" className={display.variable}>
      <body>
        {children}
        {/* Toasts for every mutation's feedback (sonner, CSS-animated). */}
        <Toaster position="bottom-right" richColors />
      </body>
    </html>
  );
}
