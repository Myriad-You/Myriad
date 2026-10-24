import { useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import * as phantasiApi from '../../services/phantasiApi'
import { formatUserFacingError } from '../../utils/formatUserFacingError'
import { useFitFrameHeight } from './embedFrame'
import './embed.css'

export function EmbedFriendApply() {
  const { t } = useI18n()
  const copy = t.embed
  const [siteName, setSiteName] = useState('')
  const [siteUrl, setSiteUrl] = useState('')
  const [feedUrl, setFeedUrl] = useState('')
  const [description, setDescription] = useState('')
  const [message, setMessage] = useState('')
  const [applicantName, setApplicantName] = useState('')
  const [applicantEmail, setApplicantEmail] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState(false)
  const fitRef = useFitFrameHeight(true)

  const submit = async () => {
    const name = siteName.trim()
    const url = siteUrl.trim()
    if (!name || !url) {
      setError(copy.required)
      return
    }
    setBusy(true)
    setError('')
    try {
      await phantasiApi.applySourceApplication({
        site_name: name,
        site_url: url,
        feed_url: feedUrl.trim() || undefined,
        description: description.trim() || undefined,
        message: message.trim() || undefined,
        applicant_name: applicantName.trim() || undefined,
        applicant_email: applicantEmail.trim() || undefined,
      })
      setDone(true)
    } catch (err) {
      setError(await formatUserFacingError(err, copy.submitFailed))
    } finally {
      setBusy(false)
    }
  }

  if (done) {
    return (
      <main ref={fitRef} className="embed-root embed-root--fit">
        <h1>{copy.friendTitle}</h1>
        <p>{copy.submitted}</p>
        <button
          type="button"
          onClick={() => {
            setDone(false)
            setSiteName('')
            setSiteUrl('')
            setFeedUrl('')
            setDescription('')
            setMessage('')
            setApplicantName('')
            setApplicantEmail('')
          }}
        >
          {copy.again}
        </button>
      </main>
    )
  }

  return (
    <main ref={fitRef} className="embed-root embed-root--fit">
      <h1>{copy.friendTitle}</h1>
      <p>{copy.friendHint}</p>
      {error ? <p className="embed-error" role="alert">{error}</p> : null}
      <form
        className="embed-fields"
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <label>
          {copy.siteName}
          <input value={siteName} required onChange={(event) => setSiteName(event.target.value)} />
        </label>
        <label>
          {copy.siteUrl}
          <input value={siteUrl} required type="url" onChange={(event) => setSiteUrl(event.target.value)} />
        </label>
        <label>
          {copy.feedUrl}
          <input value={feedUrl} onChange={(event) => setFeedUrl(event.target.value)} />
        </label>
        <label>
          {copy.description}
          <textarea rows={3} value={description} onChange={(event) => setDescription(event.target.value)} />
        </label>
        <label>
          {copy.message}
          <textarea rows={3} value={message} onChange={(event) => setMessage(event.target.value)} />
        </label>
        <label>
          {copy.applicantName}
          <input value={applicantName} onChange={(event) => setApplicantName(event.target.value)} />
        </label>
        <label>
          {copy.applicantEmail}
          <input value={applicantEmail} type="email" onChange={(event) => setApplicantEmail(event.target.value)} />
        </label>
        <button type="submit" disabled={busy}>{busy ? copy.sending : copy.submit}</button>
      </form>
    </main>
  )
}
