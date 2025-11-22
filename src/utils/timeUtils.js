const GMT_OFFSET = 5.5 * 60 * 60 * 1000; // GMT+5:30
const RESET_HOUR = 20; // 8 PM

function getDateString(timestamp) {
  const localTime = new Date(timestamp + GMT_OFFSET);
  return localTime.toISOString().split('T')[0];
}

function getYesterdayDateString() {
  const yesterday = Date.now() - (24 * 60 * 60 * 1000);
  return getDateString(yesterday);
}

function getDaysDifference(dateString1, dateString2) {
  const date1 = new Date(dateString1 + 'T00:00:00Z');
  const date2 = new Date(dateString2 + 'T00:00:00Z');
  const diffTime = Math.abs(date2.getTime() - date1.getTime());
  return Math.floor(diffTime / (24 * 60 * 60 * 1000));
}

function shouldReset(lastResetTimestamp) {
  const now = new Date();
  const lastReset = new Date(lastResetTimestamp);
  
  const currentLocalTime = new Date(now.getTime() + GMT_OFFSET);
  const lastResetLocalTime = new Date(lastReset.getTime() + GMT_OFFSET);
  
  const currentResetPoint = new Date(currentLocalTime);
  currentResetPoint.setHours(RESET_HOUR, 0, 0, 0);
  if (currentLocalTime.getHours() < RESET_HOUR) {
    currentResetPoint.setDate(currentResetPoint.getDate() - 1);
  }
  
  const lastResetPoint = new Date(lastResetLocalTime);
  lastResetPoint.setHours(RESET_HOUR, 0, 0, 0);
  if (lastResetLocalTime.getHours() < RESET_HOUR) {
    lastResetPoint.setDate(lastResetPoint.getDate() - 1);
  }
  
  return currentResetPoint.getTime() > lastResetPoint.getTime();
}

module.exports = {
  getDateString,
  getYesterdayDateString,
  getDaysDifference,
  shouldReset,
  GMT_OFFSET
};
