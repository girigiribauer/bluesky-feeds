import { Component } from 'solid-js';

const Login: Component = () => {
    return (
        <div class="auth-card">
            <h1>Private List</h1>
            <a href="/oauth/login" class="btn">
                Login with Bluesky
            </a>
        </div>
    );
};

export default Login;
