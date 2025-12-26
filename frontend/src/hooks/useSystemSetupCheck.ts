import { useEffect } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';
import { API_URL } from '../config';
import { useNotification } from '../contexts/NotificationContext';

export function useSystemSetupCheck() {
  const location = useLocation();
  const navigate = useNavigate();
  const { showError } = useNotification();

  useEffect(() => {
    if (location.pathname === '/setup') return;

    async function checkSetup() {
      try {
        const response = await fetch(`${API_URL}/api/setup/status`);
        if (!response.ok) return;

        const data = await response.json();
        if (data.is_setup_required) {
          console.warn('System not setup, redirecting to setup wizard...');
          // Use hard redirect to avoid conflicts with router/animations during init
          window.location.replace('/setup');
        }
      } catch (error) {
        console.error('Failed to check setup status:', error);
        showError('无法连接到服务器检查系统状态');
      }
    }

    checkSetup();
  }, [location.pathname, navigate, showError]);
}
