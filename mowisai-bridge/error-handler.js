// Enhanced Error Handler Module
// Provides detailed error messages with traces and suggestions

export class ErrorHandler {
  constructor() {
    this.errors = [];
    this.errorPatterns = {
      'timeout': {
        suggestion: 'The task exceeded the timeout limit. Try: 1) Breaking into smaller tasks 2) Increasing timeout 3) Simplifying the request',
        severity: 'high'
      },
      'memory': {
        suggestion: 'Memory limit exceeded. Try: 1) Increase memory_mb config 2) Process data in chunks 3) Clear intermediate results',
        severity: 'critical'
      },
      'socket': {
        suggestion: 'Connection error. Ensure: 1) MowisAI Engine is running 2) Socket path is correct 3) Permissions are set',
        severity: 'high'
      },
      'permission': {
        suggestion: 'Permission denied. Check: 1) File/directory permissions 2) User has required access 3) SELinux/AppArmor policies',
        severity: 'high'
      },
      'file_not_found': {
        suggestion: 'File not found. Verify: 1) File path is correct 2) File exists in sandbox 3) Path is absolute',
        severity: 'medium'
      },
      'syntax': {
        suggestion: 'Syntax error in code. Review: 1) Code grammar 2) Bracket matching 3) Indentation',
        severity: 'medium'
      },
      'network': {
        suggestion: 'Network error. Check: 1) Internet connection 2) Firewall settings 3) API endpoint availability',
        severity: 'high'
      },
      'api_key': {
        suggestion: 'API key error. Fix: 1) Set correct API key 2) Check key expiration 3) Verify key permissions',
        severity: 'critical'
      }
    };
  }

  categorizeError(error) {
    const message = error.message?.toLowerCase() || String(error).toLowerCase();
    
    for (const [keyword, pattern] of Object.entries(this.errorPatterns)) {
      if (message.includes(keyword) || message.includes(keyword.replace('_', ' '))) {
        return keyword;
      }
    }
    
    return 'unknown';
  }

  formatError(error, context = {}) {
    const category = this.categorizeError(error);
    const pattern = this.errorPatterns[category] || { 
      suggestion: 'Check logs for more details', 
      severity: 'unknown' 
    };

    const errorObj = {
      timestamp: new Date().toISOString(),
      type: error.name || 'Error',
      message: error.message || String(error),
      category,
      severity: pattern.severity,
      suggestion: pattern.suggestion,
      context,
      stack: error.stack ? error.stack.split('\n') : [],
      code: error.code || null,
      statusCode: error.statusCode || null
    };

    this.errors.push(errorObj);

    // Keep last 100 errors
    if (this.errors.length > 100) {
      this.errors.shift();
    }

    return errorObj;
  }

  formatDetailedError(error, additionalInfo = {}) {
    const formatted = this.formatError(error, additionalInfo);

    return `
╔════════════════════════════════════════╗
║ ERROR REPORT                           ║
╚════════════════════════════════════════╝

Type: ${formatted.type}
Severity: ${formatted.severity.toUpperCase()}
Category: ${formatted.category}
Timestamp: ${formatted.timestamp}

Message:
${formatted.message}

${formatted.code ? `Code: ${formatted.code}\n` : ''}${formatted.statusCode ? `Status Code: ${formatted.statusCode}\n` : ''}
Stack Trace:
${formatted.stack.slice(0, 10).map(line => '  ' + line.trim()).join('\n')}

Suggestion:
${formatted.suggestion}

${Object.keys(additionalInfo).length > 0 ? `Context: ${JSON.stringify(additionalInfo, null, 2)}\n` : ''}
════════════════════════════════════════
    `;
  }

  getErrorSummary(limit = 20) {
    return this.errors.slice(-limit).map(err => ({
      timestamp: err.timestamp,
      type: err.type,
      message: err.message,
      category: err.category,
      severity: err.severity
    }));
  }

  getAllErrors() {
    return this.errors;
  }

  clearErrors() {
    this.errors = [];
  }

  getErrorStats() {
    const stats = {
      total: this.errors.length,
      bySeverity: { critical: 0, high: 0, medium: 0, low: 0, unknown: 0 },
      byCategory: {},
      byType: {}
    };

    this.errors.forEach(err => {
      stats.bySeverity[err.severity]++;
      stats.byCategory[err.category] = (stats.byCategory[err.category] || 0) + 1;
      stats.byType[err.type] = (stats.byType[err.type] || 0) + 1;
    });

    return stats;
  }

  suggestFix(error) {
    const category = this.categorizeError(error);
    const pattern = this.errorPatterns[category];
    
    if (!pattern) {
      return 'Unknown error. Check logs and try again.';
    }

    return pattern.suggestion;
  }
}

// Export singleton instance
export const errorHandler = new ErrorHandler();
