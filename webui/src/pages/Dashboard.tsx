import { Component } from 'solid-js';

interface DashboardProps {
    did: string;
}

const Dashboard: Component<DashboardProps> = (props) => {
    return (
        <div class="auth-card">
            <h1>Dashboard</h1>
            <div class="did-display">
                {props.did}
            </div>
            <p>まだなーんもないよ</p>
            <a href="/oauth/logout" class="btn btn-secondary">
                Logout
            </a>
        </div>
    );
};

export default Dashboard;
