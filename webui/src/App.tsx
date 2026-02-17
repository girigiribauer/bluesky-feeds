import { Component, createResource, Show } from 'solid-js';

const fetchUser = async () => {
  const response = await fetch('/privatelist/me');
  if (!response.ok) return null;
  return response.json() as Promise<{ did: string }>;
};

const App: Component = () => {
  const [user] = createResource(fetchUser);

  return (
    <div class="card">
      <h1>Private List</h1>
      <p>Management Platform</p>

      <Show 
        when={!user.loading} 
        fallback={<div class="loading">Initializing...</div>}
      >
        <Show 
          when={user()} 
          fallback={
            <div>
              <a href="/oauth/login" class="btn">
                <span>Login with Bluesky</span>
              </a>
            </div>
          }
        >
          {(u) => (
            <div>
              <div class="user-info">
                {u().did}
              </div>
              <a href="/oauth/logout" class="btn btn-secondary">
                Logout
              </a>
            </div>
          )}
        </Show>
      </Show>
    </div>
  );
};

export default App;
