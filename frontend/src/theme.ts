import { createTheme } from '@mui/material/styles';

const heading = {
  fontWeight: 800,
  letterSpacing: 0
};

export const theme = createTheme({
  palette: {
    primary: {
      main: '#4D006E',
      contrastText: '#FFFFFF'
    },
    secondary: {
      main: '#FFC107',
      contrastText: '#2A1034'
    },
    background: {
      default: '#F7F2FA',
      paper: '#FFFFFF'
    },
    text: {
      primary: '#211527',
      secondary: '#65536D'
    }
  },
  shape: {
    borderRadius: 8
  },
  typography: {
    fontFamily:
      'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
    h1: heading,
    h2: heading,
    h3: heading,
    h4: heading,
    h5: heading,
    h6: heading,
    button: {
      textTransform: 'none',
      fontWeight: 700,
      letterSpacing: 0
    }
  },
  components: {
    MuiButton: {
      styleOverrides: {
        root: {
          minHeight: 40,
          boxShadow: 'none'
        }
      }
    },
    MuiPaper: {
      styleOverrides: {
        root: {
          backgroundImage: 'none'
        }
      }
    },
    MuiChip: {
      styleOverrides: {
        root: {
          fontWeight: 700,
          letterSpacing: 0
        }
      }
    }
  }
});
