import {
  createContext, useCallback, useContext, useMemo, type ReactNode,
} from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { api, jsonBody } from './api';
import { qk } from './queryKeys';
import type { SetupStatus, User } from './types';

interface AuthValue {
  user: User | null;
  setupRequired: boolean;
  loading: boolean;
  login: (username: string, password: string) => Promise<void>;
  setup: (username: string, displayName: string, password: string) => Promise<void>;
  logout: () => Promise<void>;
}

const AuthContext = createContext<AuthValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const client = useQueryClient();

  const setupStatus = useQuery({
    queryKey: qk.auth.setupStatus(),
    queryFn: () => api<SetupStatus>('/auth/setup-status'),
    staleTime: 60_000,
  });

  // Only ask who we are once we know the instance has been set up at all;
  // otherwise the first load always logs a pointless 401.
  const me = useQuery({
    queryKey: qk.auth.me(),
    queryFn: () => api<User>('/auth/me'),
    enabled: setupStatus.data?.setupRequired === false,
    retry: false,
    staleTime: 60_000,
  });

  const loginMutation = useMutation({
    mutationFn: (body: { username: string; password: string }) =>
      api<User>('/auth/login', { method: 'POST', ...jsonBody(body) }),
    onSuccess: (user) => client.setQueryData(qk.auth.me(), user),
  });

  const setupMutation = useMutation({
    mutationFn: (body: { username: string; displayName: string; password: string }) =>
      api<User>('/auth/setup', { method: 'POST', ...jsonBody(body) }),
    onSuccess: (user) => {
      client.setQueryData(qk.auth.me(), user);
      client.invalidateQueries({ queryKey: qk.auth.setupStatus() });
    },
  });

  const logoutMutation = useMutation({
    mutationFn: () => api<void>('/auth/logout', { method: 'POST' }),
    // Everything in the cache belongs to the session that just ended.
    onSuccess: () => client.clear(),
  });

  const login = useCallback(
    async (username: string, password: string) => {
      await loginMutation.mutateAsync({ username, password });
    },
    [loginMutation],
  );

  const setup = useCallback(
    async (username: string, displayName: string, password: string) => {
      await setupMutation.mutateAsync({ username, displayName, password });
    },
    [setupMutation],
  );

  const logout = useCallback(async () => {
    await logoutMutation.mutateAsync();
  }, [logoutMutation]);

  const value = useMemo<AuthValue>(
    () => ({
      user: me.data ?? null,
      setupRequired: setupStatus.data?.setupRequired ?? false,
      loading: setupStatus.isLoading || (me.isLoading && me.fetchStatus !== 'idle'),
      login,
      setup,
      logout,
    }),
    [me.data, me.isLoading, me.fetchStatus, setupStatus.data, setupStatus.isLoading, login, setup, logout],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthValue {
  const value = useContext(AuthContext);
  if (!value) throw new Error('useAuth must be used inside AuthProvider');
  return value;
}
