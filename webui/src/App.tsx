import { Component, createResource, Show } from 'solid-js';
import Login from './pages/Login';
import Dashboard from './pages/Dashboard';

const fetchUser = async () => {
  const response = await fetch('/privatelist/me');
  if (!response.ok) return null;
  return response.json() as Promise<{ did: string }>;
};

const App: Component = () => {
  const [user] = createResource(fetchUser);

  return (
    <Show
      when={!user.loading}
      fallback={<div class="loading">Loading...</div>}
    >
      <Show
        when={user()}
        fallback={<Login />}
      >
        {(u) => <Dashboard did={u().did} />}
      </Show>
    </Show>
  );
};

export default App;
