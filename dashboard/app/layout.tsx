import React from 'react';
import { Providers } from '../lib/query';

export const metadata = {
  title: 'Deploy Platform',
  description: 'Single-node deployment platform control plane',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body style={{ margin: 0, padding: 0, backgroundColor: '#f9fafb' }}>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
